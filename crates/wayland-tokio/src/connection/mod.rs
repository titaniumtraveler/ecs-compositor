use crate::{dir::Client, drive_io::Io};
use ecs_compositor_core::{Interface, new_id, new_id_dyn, object, string, uint};
use std::{
    env, io,
    marker::PhantomData,
    num::{NonZero, NonZeroU32},
    os::{
        fd::{AsRawFd, RawFd},
        unix::net::UnixStream,
    },
    path::PathBuf,
    ptr::NonNull,
    sync::{Mutex, MutexGuard, TryLockError},
};
use tokio::io::unix::AsyncFd;

mod obj;
mod registry;

pub use self::obj::Object;
pub(crate) use self::registry::Registry;

pub struct Connection<Dir> {
    pub(crate) fd: AsyncFd<UnixStream>,
    drive_io: Mutex<Io>,
    registry: Mutex<Registry<Dir>>,
}

impl<Dir> Connection<Dir> {
    pub fn new() -> io::Result<Self> {
        let sock = UnixStream::connect(PathBuf::from_iter([
            env::var_os("XDG_RUNTIME_DIR").unwrap(),
            env::var_os("WAYLAND_DISPLAY").unwrap(),
        ]))?;

        Ok(Self {
            fd: AsyncFd::new(sock)?,
            drive_io: Mutex::new(Io::new()),
            registry: Mutex::new(Registry::new()),
        })
    }

    fn registry(&self) -> MutexGuard<'_, Registry<Dir>> {
        self.registry.lock().unwrap()
    }

    pub(crate) fn try_lock_io_buf(&self) -> Option<MutexGuard<'_, Io>> {
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
    /// # Panic
    /// Does panic if `id` is `0`.
    fn new_object_with_id<I>(&self, id: u32) -> Object<Self, I, Client>
    where
        I: Interface,
    {
        Object {
            conn: self.clone(),
            id: object {
                id: NonZero::new(id).unwrap(),
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
