use crate::{
    connection::{Client, Connection, Object},
    dir::InterfaceDir,
};
use ecs_compositor_core::{Interface, object};
use std::{
    collections::{BTreeMap, VecDeque, btree_map},
    marker::PhantomData,
    num::NonZeroU32,
    sync::MutexGuard,
    task::{Context, Waker},
};
use tracing::{instrument, trace};

pub(crate) struct Registry<Dir> {
    next_id: NonZeroU32,
    pub(crate) receiver_map: BTreeMap<object, RecvEntry>,
    sender_queue: VecDeque<Waker>,
    sender_locked: Option<Waker>,
    dir: PhantomData<Dir>,
}

pub(crate) struct RecvEntry {
    pub(crate) waker: Waker,
    pub(crate) fd_count: fn(u16) -> Option<usize>,
}

impl<Dir> Registry<Dir> {
    pub(crate) fn new() -> Self {
        Self {
            receiver_map: BTreeMap::new(),
            sender_queue: VecDeque::new(),
            next_id: NonZeroU32::new(2).unwrap(),
            sender_locked: None,
            dir: PhantomData,
        }
    }
}

impl Registry<Client> {
    pub(crate) fn new_object<Conn, I>(&mut self, conn: Conn) -> Object<Conn, I, Client>
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

impl<Dir> Registry<Dir> {
    #[instrument(level = "trace", skip_all)]
    pub(crate) fn register_recv<I>(&mut self, obj: object<I>, cx: &mut Context<'_>)
    where
        I: Interface,
        Dir: InterfaceDir<I>,
    {
        match self.receiver_map.entry(obj.cast::<()>()) {
            btree_map::Entry::Vacant(vacant_entry) => {
                trace!(id = obj.id, "register new recv");
                vacant_entry.insert(RecvEntry {
                    waker: cx.waker().clone(),
                    fd_count: <Dir as InterfaceDir<I>>::recv_fd_count,
                });
            }
            btree_map::Entry::Occupied(occupied_entry) => {
                trace!(id = obj.id, "reregister old recv");
                occupied_entry.into_mut().waker.clone_from(cx.waker());
            }
        }
    }

    #[instrument(level = "trace", skip_all)]
    fn register_send(&mut self, cx: &mut Context<'_>) {
        self.sender_queue.push_back(cx.waker().clone());
    }

    fn register_send_locked(&mut self, cx: &mut Context<'_>) {
        match &mut self.sender_locked {
            locked @ None => *locked = Some(cx.waker().clone()),
            Some(_) => self.sender_queue.push_back(cx.waker().clone()),
        }
    }

    fn wake_sender(&mut self) -> bool {
        if let Some(waker) = self.sender_locked.take() {
            waker.wake();
            true
        } else {
            self.sender_queue.pop_front().map(Waker::wake).is_some()
        }
    }

    fn wake_recver(&mut self, cx: &mut Context<'_>) {
        if let Some(waker) = self.sender_locked.take() {
            waker.wake();
        }

        if let Some(waker) = self.receiver_map.first_entry() {
            let waker = &waker.get().waker;
            if !waker.will_wake(cx.waker()) {
                waker.wake_by_ref();
            }
        }
    }
}

impl<Conn, I, Dir> Object<Conn, I, Dir>
where
    Conn: AsRef<Connection<Dir>>,
    I: Interface,
    Dir: InterfaceDir<I>,
{
    pub(crate) fn conn(&self) -> &Connection<Dir> {
        self.conn.as_ref()
    }

    pub(crate) fn registry(&self) -> MutexGuard<'_, Registry<Dir>> {
        self.conn.as_ref().registry.lock().unwrap()
    }

    pub(crate) fn register_recv(&self, cx: &mut Context<'_>) {
        self.registry().register_recv(self.id, cx);
    }

    pub(crate) fn register_send(&self, cx: &mut Context<'_>) {
        self.registry().register_send(cx);
    }

    pub(crate) fn register_send_locked(&self, cx: &mut Context<'_>) {
        self.registry().register_send_locked(cx);
    }

    pub(crate) fn wake_recver(&self, cx: &mut Context<'_>) {
        self.registry().wake_recver(cx)
    }

    pub(crate) fn wake_sender(&self) -> bool {
        self.registry().wake_sender()
    }
}
