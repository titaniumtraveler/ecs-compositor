use crate::protocols::wayland::wl_display::wl_display;
use anyhow::Result;
use ecs_compositor_core::{Interface, Opcode, new_id, new_id_dyn, object, string, uint};
use std::{
    collections::{BTreeMap, VecDeque, btree_map},
    env,
    fmt::{Debug, Display},
    marker::PhantomData,
    num::{NonZero, NonZeroU32},
    os::{
        fd::{AsRawFd, RawFd},
        unix::net::UnixStream,
    },
    path::PathBuf,
    ptr::NonNull,
    sync::{Mutex, MutexGuard, TryLockError},
    task::{Context, Waker},
};
use tokio::io::unix::AsyncFd;

pub use self::{drive_io::Io, recv::Recv, send::Send};

mod drive_io;
mod msg_io;
mod ready_fut;
mod recv;
mod send;

pub struct Connection<Dir> {
    fd: AsyncFd<UnixStream>,
    drive_io: Mutex<Io>,
    registry: Mutex<Registry<Dir>>,
}

impl<Dir> Connection<Dir> {
    pub fn new() -> Result<Self> {
        let sock = UnixStream::connect(PathBuf::from_iter([
            env::var_os("XDG_RUNTIME_DIR").unwrap(),
            env::var_os("WAYLAND_DISPLAY").unwrap(),
        ]))?;

        Ok(Self {
            fd: AsyncFd::new(sock)?,
            drive_io: Mutex::new(Io::new()?),
            registry: Mutex::new(Registry {
                receiver_map: BTreeMap::new(),
                sender_queue: VecDeque::new(),
                next_id: NonZeroU32::new(2).unwrap(),
                sender_locked: None,
                dir: PhantomData,
            }),
        })
    }

    fn registry(&self) -> MutexGuard<'_, Registry<Dir>> {
        self.registry.lock().unwrap()
    }

    fn try_lock_io_buf(&self) -> Option<MutexGuard<'_, Io>> {
        match self.drive_io.try_lock() {
            Ok(guard) => Some(guard),
            Err(TryLockError::WouldBlock) => None,
            Err(poison @ TryLockError::Poisoned(_)) => panic!("{:?}", poison),
        }
    }
}

impl<Dir> AsRawFd for Connection<Dir> {
    fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}

pub trait ClientHandle: AsRef<Connection<Client>> + Clone {
    fn wl_display(&self) -> Object<Self, wl_display, Client> {
        Object {
            conn: self.clone(),
            id: object {
                id: const { NonZero::new(1).unwrap() },
                _marker: PhantomData,
            },
            marker: PhantomData,
        }
    }

    fn new_object<I>(&self) -> (new_id<I>, Object<Self, I, Client>)
    where
        I: Interface,
    {
        let obj = self.as_ref().registry().new_object(self.clone());
        (obj.id.to_new_id(), obj)
    }

    fn new_object_dyn<I>(&self) -> (new_id_dyn<'static>, Object<Self, I, Client>)
    where
        I: Interface,
    {
        let obj = self.as_ref().registry().new_object(self.clone());
        (
            new_id_dyn {
                name: string {
                    ptr: Some(NonNull::from_ref(I::NAME.as_bytes()).cast()),
                    len: NonZeroU32::new(I::NAME.len() as u32).unwrap(),
                    _marker: PhantomData,
                },
                version: uint(I::VERSION),
                id: obj.id.to_new_id().cast(),
            },
            obj,
        )
    }
}

impl<Conn: AsRef<Connection<Client>> + Clone> ClientHandle for Conn {}

impl<Dir> AsRef<Connection<Dir>> for &Connection<Dir> {
    fn as_ref(&self) -> Self {
        self
    }
}

pub trait InterfaceDir<I: Interface> {
    type Recv: Opcode;
    type Send: Opcode;

    fn recv_fd_count(i: u16) -> Option<usize> {
        Self::Recv::from_u16(i).ok().as_ref().map(Opcode::fd_count)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Client;
#[derive(Debug, Clone, Copy)]
pub struct Server;

impl<I: Interface> InterfaceDir<I> for Client {
    type Recv = I::Event;
    type Send = I::Request;
}

impl<I: Interface> InterfaceDir<I> for Server {
    type Recv = I::Request;
    type Send = I::Event;
}

struct Registry<Dir> {
    next_id: NonZeroU32,
    receiver_map: BTreeMap<object, Entry>,
    sender_queue: VecDeque<Waker>,
    sender_locked: Option<Waker>,
    dir: PhantomData<Dir>,
}

impl Registry<Client> {
    fn new_object<Conn, I>(&mut self, conn: Conn) -> Object<Conn, I, Client>
    where
        Conn: AsRef<Connection<Client>>,
        I: Interface,
    {
        Object {
            conn,
            id: {
                let next_id = self.next_id;
                self.next_id = self.next_id.saturating_add(1);
                object {
                    id: next_id,
                    _marker: PhantomData,
                }
            },
            marker: PhantomData,
        }
    }
}

#[derive(Debug)]
pub struct Object<Conn, I, Dir>
where
    Conn: AsRef<Connection<Dir>>,
    I: Interface,
    Dir: InterfaceDir<I>,
{
    conn: Conn,
    id: object<I>,
    marker: PhantomData<Dir>,
}

impl<Conn, I, Dir> Display for Object<Conn, I, Dir>
where
    Conn: AsRef<Connection<Dir>>,
    I: Interface,
    Dir: InterfaceDir<I>,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!(
            "{name}:v{version}#{id}",
            name = I::NAME,
            version = I::VERSION,
            id = self.id.id
        ))
    }
}

impl<Conn, I, Dir> Clone for Object<Conn, I, Dir>
where
    Conn: AsRef<Connection<Dir>> + Clone,
    I: Interface,
    Dir: InterfaceDir<I>,
{
    fn clone(&self) -> Self {
        Self {
            conn: self.conn.clone(),
            id: self.id,
            marker: self.marker,
        }
    }
}

impl<Conn, I, Dir> Object<Conn, I, Dir>
where
    Conn: AsRef<Connection<Dir>>,
    I: Interface,
    Dir: InterfaceDir<I>,
{
    // fn recv<'a>(&'a self, guard: &mut MutexGuard<'a, Io>) {}

    fn registry(&self) -> MutexGuard<'_, Registry<Dir>> {
        self.conn.as_ref().registry.lock().unwrap()
    }

    fn register_recv(&self, cx: &mut Context<'_>) {
        match self.registry().receiver_map.entry(self.id.cast::<()>()) {
            btree_map::Entry::Vacant(vacant_entry) => {
                vacant_entry.insert(Entry {
                    waker: cx.waker().clone(),
                    fd_count: <Dir as InterfaceDir<I>>::recv_fd_count,
                });
            }
            btree_map::Entry::Occupied(occupied_entry) => {
                occupied_entry.into_mut().waker.clone_from(cx.waker());
            }
        }
    }

    fn register_send(&self, cx: &mut Context<'_>) {
        self.registry().sender_queue.push_back(cx.waker().clone());
    }

    fn register_send_locked(&self, cx: &mut Context<'_>) {
        let mut registry = self.registry();
        match &mut registry.sender_locked {
            locked @ None => *locked = Some(cx.waker().clone()),
            Some(_) => registry.sender_queue.push_back(cx.waker().clone()),
        }
    }

    fn wake_sender(&self) -> bool {
        let mut registry = self.registry();
        if let Some(waker) = registry.sender_locked.take() {
            waker.wake();
            true
        } else {
            registry.sender_queue.pop_front().map(Waker::wake).is_some()
        }
    }
}

struct Entry {
    waker: Waker,
    fd_count: fn(u16) -> Option<usize>,
}
