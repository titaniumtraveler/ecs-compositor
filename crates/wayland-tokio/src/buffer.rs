use crate::{
    connection::Object,
    drive_io::{MAX_FDS, WAYLAND_MAX_MESSAGE_LEN},
    handle::{ConnectionHandle, InterfaceDir},
    msg_io::{cmsg_cursor::CmsgCursor, recvmsg},
};
use bitvec::array::BitArray;
use ecs_compositor_core::{Interface, RawSliceExt, Value, message_header, new_id};
use futures::Stream;
use heapless::Deque;
use libc::{CMSG_SPACE, MSG_DONTWAIT, SCM_RIGHTS, SOL_SOCKET, cmsghdr, iovec};
use std::{
    alloc::Layout,
    collections::BTreeMap,
    convert::Infallible,
    io,
    marker::{PhantomData, PhantomPinned},
    ops::{ControlFlow, Range},
    os::fd::{AsRawFd, RawFd},
    pin::Pin,
    ptr::{NonNull, null_mut, slice_from_raw_parts_mut},
    sync::{
        Mutex, MutexGuard, TryLockError,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    task::{Context, Poll, Waker},
};
use tokio::io::{Ready, unix::AsyncFdReadyGuard};
use tracing::{trace, warn};

impl<Conn, I> Object<Conn, I>
where
    Conn: ConnectionHandle<Dir: InterfaceDir<I>>,
    I: Interface,
{
    pub fn recv_stream(&self) -> RecvStream<'_, Conn, I> {
        RecvStream {
            obj: self,
            recv: Default::default(),
            is_registered: false,
            _marker: PhantomData,
        }
    }
}

pub struct RecvStream<'a, Conn, I>
where
    Conn: ConnectionHandle<Dir: InterfaceDir<I>>,
    I: Interface,
{
    obj: &'a Object<Conn, I>,
    recv: Mutex<Recv>,
    is_registered: bool,
    _marker: PhantomData<(I, PhantomPinned)>,
}

impl<Conn, I> Stream for RecvStream<'_, Conn, I>
where
    Conn: ConnectionHandle<Dir: InterfaceDir<I>>,
    I: Interface,
{
    type Item = io::Result<Handle>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut s = self.as_mut();
        let obj = s.obj;
        let conn = obj.conn.conn();

        let (mut fd, waker) = {
            let mut guard = if !s.is_registered {
                s.as_mut().register()
            } else {
                match s.recv.try_lock() {
                    Ok(guard) => guard,
                    Err(TryLockError::WouldBlock) => {
                        // The tasks state is currently busy so we return so we are returning
                        // `Poll::Pending` and rely on the task that is currently working on receiving
                        // data to wake us up.
                        //
                        // Note that this *could* cause this future to hang, as we are not going to
                        // wakeup the task with its latest `Waker` (because we don't have anywhere to
                        // store it).
                        //
                        // See `Future::poll`:
                        // > Note that on multiple calls to poll, only the Waker from the Context
                        // > passed to the most recent call should be scheduled to receive a wakeup.
                        //
                        // If that were to become a real problem, a channel or something similar it
                        // could be used to make sure the wakers are *actually* awoken in cases like
                        // this.
                        trace!("task currently busy");
                        return Poll::Pending;
                    }
                    Err(TryLockError::Poisoned(err)) => panic!("{err}"),
                }
            };

            let waker = match guard.waker.take() {
                Some(mut old) => {
                    cx.waker().clone_into(&mut old);
                    old
                }
                None => cx.waker().clone(),
            };

            if let Some(handle) = guard.queue.pop_front() {
                guard.waker = Some(waker);
                return Poll::Ready(Some(Ok(handle)));
            }

            'fd: {
                let res = match conn.fd.poll_read_ready(cx) {
                    Poll::Ready(Ok(ok)) => {
                        break 'fd (ok, waker);
                    }
                    Poll::Ready(Err(err)) => Poll::Ready(Some(Err(err))),
                    Poll::Pending => Poll::Pending,
                };
                guard.waker = Some(waker);

                return res;
            }
        };

        let mut state = conn.recv.state.lock().unwrap();
        let res = conn.recv.recv(&mut state, &mut fd);

        let mut guard = match s.recv.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::WouldBlock) => {
                // The local lock should never be locked by anyone else than the owner, or holder
                // of `RecvBuf::state`, and we are holding the `RecvBuf::state` guard currently.
                unreachable!("mutex locked incorrectly")
            }
            Err(TryLockError::Poisoned(err)) => panic!("{err}"),
        };

        debug_assert!(guard.waker.is_none());
        guard.waker = Some(waker);

        () = res?;

        if let Some(val) = guard.queue.pop_front() {
            return Poll::Ready(Some(Ok(val)));
        }

        // The stream will be woken up when another stream ends up driving the io
        // and then gives us the message handle
        Poll::Pending
    }
}

impl<Conn, I> RecvStream<'_, Conn, I>
where
    Conn: ConnectionHandle<Dir: InterfaceDir<I>>,
    I: Interface,
{
    fn register<'a>(self: Pin<&'a mut Self>) -> MutexGuard<'a, Recv> {
        // Safety:
        // This takes a reference `&'a self.recv`, casts it to `&'static` and stores that reference
        // in the receiver map.
        //
        // This is safe, because when `Drop`ing `Self`, this `&'static` reference is removed again
        // and the drop is guarantied to happen, because `self` is pinned.
        unsafe {
            let s = Pin::into_inner_unchecked(self);
            let recv: &'a Mutex<Recv> = &s.recv;
            let guard = recv
                .try_lock()
                .expect("when the stream is not registered, this mutex should **never** be locked");

            s.is_registered = true;

            s.obj.conn().recv.state.lock().unwrap().map.insert(
                s.obj.id().cast().to_new_id(),
                Entry {
                    recv: cast_to_static::<Mutex<Recv>>(recv),
                    fd_count: <Conn::Dir>::recv_fd_count,
                },
            );

            guard
        }
    }

    fn deregister(self: Pin<&mut Self>) {
        unsafe {
            let s = Pin::into_inner_unchecked(self);

            s.obj
                .conn()
                .recv
                .state
                .lock()
                .unwrap()
                .map
                .remove(&s.obj.id().cast().to_new_id());

            s.is_registered = false;
        }
    }
}

impl<Conn, I> Drop for RecvStream<'_, Conn, I>
where
    Conn: ConnectionHandle<Dir: InterfaceDir<I>>,
    I: Interface,
{
    fn drop(&mut self) {
        inner_drop(unsafe { Pin::new_unchecked(self) });

        fn inner_drop<Conn, I>(s: Pin<&mut RecvStream<Conn, I>>)
        where
            Conn: ConnectionHandle<Dir: InterfaceDir<I>>,
            I: Interface,
        {
            if s.is_registered {
                s.deregister();
            }
        }
    }
}

unsafe fn cast_to_static<'a, T>(val: &'a T) -> &'static T {
    unsafe { std::mem::transmute::<&'a T, &'static T>(val) }
}

pub struct RecvBuf {
    slot: BitArray<[AtomicU64; 1]>, // maximum would be 512
    data: NonNull<u8>,
    ctrl: NonNull<RawFd>,

    slot_free: AtomicUsize,
    slot_next: AtomicUsize,

    data_free: AtomicUsize,
    data_next: AtomicUsize,

    ctrl_free: AtomicUsize,
    ctrl_next: AtomicUsize,

    /// TODO: decide on the locking semantics
    /// - Streams when they first register require quick and potentially time sensitive access waiting for the lock could the runtime for a bit
    /// - When `self.recv()`ing, we hold the lock for longer time. (Reading and parsing the wayland messages)
    ///
    /// Doing nothing won't cause a deadlock, but might be less efficient than desired
    state: Mutex<State>,
}

impl RecvBuf {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            slot: BitArray::new([AtomicU64::new(u64::MAX); _]),
            data: unsafe {
                let layout =
                    Layout::from_size_align(Self::DATA_CAPACITY, align_of::<u32>()).unwrap();
                let data = std::alloc::alloc_zeroed(layout);
                let Some(data) = NonNull::new(data) else {
                    panic!("failed to allocate buffer");
                };

                data
            },
            ctrl: unsafe {
                let layout =
                    Layout::from_size_align(Self::CTRL_CAPACITY, align_of::<RawFd>()).unwrap();
                let ctrl = std::alloc::alloc_zeroed(layout);
                let Some(ctrl) = NonNull::new(ctrl) else {
                    panic!("failed to allocate buffer");
                };

                ctrl.cast::<RawFd>()
            },

            slot_free: AtomicUsize::new(0),
            slot_next: AtomicUsize::new(0),

            data_free: AtomicUsize::new(0),
            data_next: AtomicUsize::new(0),

            ctrl_free: AtomicUsize::new(0),
            ctrl_next: AtomicUsize::new(0),

            state: Mutex::new(State {
                buf_state: BufState {
                    next_data: 0,
                    next_ctrl: 0,

                    over_read_data: None,
                    over_read_ctrl: None,
                },

                parsing_state: ParsingState::None,

                cmsg_buf: [0; _],
                map: Default::default(),
            }),
        }
    }
}

struct State {
    buf_state: BufState,
    parsing_state: ParsingState,

    cmsg_buf: [u8; unsafe { CMSG_SPACE(size_of::<RawFd>() as u32 * MAX_FDS) as usize }],

    map: BTreeMap<new_id, Entry>,
}

enum ParsingState {
    None,
    Header(message_header),
    Handle {
        header: message_header,
        handle: RawHandle,
        next: NextAlloc,
    },
}

struct BufState {
    next_data: usize,
    next_ctrl: usize,

    over_read_data: Option<usize>,
    over_read_ctrl: Option<usize>,
}

struct Entry {
    recv: &'static Mutex<Recv>,
    fd_count: fn(u16) -> Option<usize>,
}

impl Entry {
    fn fd_count(&self, opcode: u16) -> ControlFlow<io::Result<()>, usize> {
        match (self.fd_count)(opcode) {
            Some(val) => ControlFlow::Continue(val),
            None => ControlFlow::Break(Err(io::Error::other("invalid opcode"))),
        }
    }
}

pub struct MsgHandle<'a, Conn, I>
where
    Conn: ConnectionHandle<Dir: InterfaceDir<I>>,
    I: Interface,
{
    object: &'a Object<Conn, I>,
    handle: Handle,
}

impl<Conn, I> Drop for MsgHandle<'_, Conn, I>
where
    Conn: ConnectionHandle<Dir: InterfaceDir<I>>,
    I: Interface,
{
    fn drop(&mut self) {
        let recv = &self.object.conn().recv;
        let free = recv.slot_free.load(Ordering::Acquire);

        if self.handle.slot != free {
            recv.slot.set_aliased(self.handle.slot, false);
            return;
        }
        let next = recv.slot_next.load(Ordering::Acquire);

        assert_ne!(free, next);
        let not_reversed = free < next;
        if not_reversed {
            let slots = &recv.slot.as_bitslice();
            let end = slots[free..next]
                .first_one()
                .map(|i| free + i)
                .unwrap_or(next);

            for index in free..end {
                slots.set_aliased(index, true);
            }

            if end == next {
                todo!()
            }

            if !slots[end] {
                todo!("loop")
            }
        } else {
            todo!("reversed")
        }
    }
}

pub struct Handle {
    slot: usize,
    hdr: message_header,
    inner: RawHandle,
    next: NextAlloc,
}

#[derive(Debug, Clone, Copy)]
struct NextAlloc {
    data_next: usize,
    ctrl_next: usize,
}

#[derive(Debug, Clone, Copy)]
struct RawHandle {
    data: *mut [u8],
    ctrl: *mut [RawFd],
}

#[derive(Default)]
struct Recv {
    waker: Option<Waker>,
    queue: Deque<Handle, 16>,
}

struct B {
    slot: Pair<{ RecvBuf::SLOT_CAPACITY }>,
    data: Pair<{ RecvBuf::DATA_CAPACITY }>,
    ctrl: Pair<{ RecvBuf::CTRL_CAPACITY }>,
}

/// describes the range `free..next` + wrapping logic
struct Pair<const CAPACITY: usize> {
    /// `free` is *inclusive*
    free: usize,
    /// `next` is *exclusive*
    next: usize,
}

impl<const CAPACITY: usize> Pair<CAPACITY> {
    fn free_space(&self, hold: usize) -> Bufs {
        free_space(self.free, self.next, hold)
    }

    fn range_in_bound(&self, base: usize, len: usize) -> bool {
        debug_assert!(base + len < CAPACITY);
        let is_reversed_buf = self.free <= self.next;
        let is_base_in_bound = self.free <= base;
        let is_end_in_bound = base + len < self.next;

        (is_reversed_buf & is_base_in_bound & is_end_in_bound)
            | (!is_reversed_buf & is_base_in_bound)
            | (!is_reversed_buf & is_end_in_bound)
    }
}

impl RecvBuf {
    const DATA_CAPACITY: usize = WAYLAND_MAX_MESSAGE_LEN * 4;
    const CTRL_CAPACITY: usize = 1024;
    const SLOT_CAPACITY: usize = 64;

    const DATA_THRESHOLD: usize = Self::DATA_CAPACITY - WAYLAND_MAX_MESSAGE_LEN;
    const CTRL_THRESHOLD: usize = 1024 - 8;

    fn acquire_buf(&self) -> B {
        B {
            slot: Pair {
                free: self.slot_free.load(Ordering::Relaxed),
                next: self.slot_next.load(Ordering::Acquire),
            },

            data: Pair {
                free: self.data_free.load(Ordering::Relaxed),
                next: self.data_next.load(Ordering::Acquire),
            },

            ctrl: Pair {
                free: self.ctrl_free.load(Ordering::Relaxed),
                next: self.ctrl_next.load(Ordering::Acquire),
            },
        }
    }

    fn release_buf(&self, b: B) {
        self.slot_next.store(b.slot.next, Ordering::Release);
        self.data_next.store(b.data.next, Ordering::Release);
        self.ctrl_next.store(b.ctrl.next, Ordering::Release);
    }

    fn data_slice(
        &self,
        pair: &Pair<{ Self::DATA_CAPACITY }>,
        base: usize,
        len: impl Into<usize>,
    ) -> Option<NonNull<[u8]>> {
        let len = len.into();
        unsafe {
            if pair.range_in_bound(base, len) {
                let slice = std::ptr::slice_from_raw_parts_mut(self.data.as_ptr().add(base), len);
                Some(NonNull::new_unchecked(slice))
            } else {
                None
            }
        }
    }

    fn ctrl_slice(
        &self,
        pair: &Pair<{ Self::CTRL_CAPACITY }>,
        base: usize,
        len: usize,
    ) -> Option<NonNull<[RawFd]>> {
        unsafe {
            if pair.range_in_bound(base, len) {
                let slice = std::ptr::slice_from_raw_parts_mut(self.ctrl.as_ptr().add(base), len);
                Some(NonNull::new_unchecked(slice))
            } else {
                None
            }
        }
    }

    fn ctrl_range(&self, range: &Range<usize>) -> NonNull<[RawFd]> {
        slice_with_len(self.ctrl, range.start, range.end - range.start)
    }

    fn try_get_bufs(
        &self,
        b: &B,
        state: &mut BufState,
        data_len: usize,
        ctrl_len: usize,
    ) -> ControlFlow<(), RawHandle> {
        let Some(data) = self.data_slice(&b.data, state.next_data, data_len) else {
            let end = state.next_data + data_len;
            if Self::DATA_THRESHOLD < end {
                state.over_read_data = Some(end);
            }

            return ControlFlow::Break(());
        };
        debug_assert!(data.cast::<u32>().is_aligned());

        let Some(ctrl) = self.ctrl_slice(&b.ctrl, state.next_ctrl, ctrl_len) else {
            let end = state.next_ctrl + ctrl_len;
            if Self::CTRL_THRESHOLD < end {
                state.over_read_ctrl = Some(end);
            }

            return ControlFlow::Break(());
        };
        debug_assert!(ctrl.cast::<RawFd>().is_aligned());

        state.next_data += data_len;
        state.next_ctrl += ctrl_len;

        ControlFlow::Continue(RawHandle {
            data: data.as_ptr(),
            ctrl: ctrl.as_ptr(),
        })
    }

    fn alloc_slot(&self, slots: &mut Pair<{ Self::SLOT_CAPACITY }>) -> ControlFlow<(), usize> {
        let slot = slots.next;

        slots.next = {
            let mut next = slots.next + 1;
            if next == Self::SLOT_CAPACITY {
                next = 0
            }

            if next == slots.free {
                return ControlFlow::Break(());
            }

            next
        };

        ControlFlow::Continue(slot)
    }

    /// TODO: How to represent cases like [`tokio::io::Ready::READ_CLOSED`] or protocol errors?
    /// - errors
    ///   - invalid length
    ///   - invalid opcode
    ///   - unsupported version
    ///
    /// TODO: Add our own `WaylandError` enum
    /// TODO: Observe what kind of errors are needed by the `send` side of things.
    fn recv<T: AsRawFd>(&self, state: &mut State, fd: &mut AsyncFdReadyGuard<T>) -> io::Result<()> {
        let mut b = self.acquire_buf();

        let ControlFlow::Break(res) = (|| -> ControlFlow<_, Infallible> {
            loop {
                self.fill_buffer(&mut b, state, fd)?;

                loop {
                    match self.parse_message(&mut b, state) {
                        ControlFlow::Continue(()) => (),
                        ControlFlow::Break(Ok(())) => break,
                        ControlFlow::Break(err @ Err(_)) => return ControlFlow::Break(err),
                    }
                }
            }
        })();

        self.release_buf(b);

        res
    }

    fn fill_buffer<T: AsRawFd>(
        &self,
        b: &mut B,
        state: &mut State,
        fd: &mut AsyncFdReadyGuard<T>,
    ) -> ControlFlow<io::Result<()>, ()> {
        unsafe {
            let data = match b.data.free_space(
                state
                    .buf_state
                    .over_read_data
                    .unwrap_or(Self::DATA_THRESHOLD),
            ) {
                Bufs::None => return ControlFlow::Break(Ok(())),
                Bufs::Two(range, _) if state.buf_state.over_read_data.is_none() => Bufs::One(range),
                val => val,
            };

            let mut iovecs = [iovec {
                iov_base: null_mut(),
                iov_len: 0,
            }; 2];

            let ctrl = match b.ctrl.free_space(
                state
                    .buf_state
                    .over_read_ctrl
                    .unwrap_or(Self::CTRL_THRESHOLD),
            ) {
                Bufs::One(range) if MAX_FDS as usize <= range.end - range.start => Bufs::One(range),
                Bufs::Two(range, _)
                    if state.buf_state.over_read_ctrl.is_none()
                        && MAX_FDS as usize <= (range.end - range.start) =>
                {
                    Bufs::One(range)
                }
                Bufs::Two(range1, range2)
                    if MAX_FDS as usize
                        <= ((range1.end - range1.start) + (range2.end - range2.start)) =>
                {
                    Bufs::Two(range1, range2)
                }
                _ => return ControlFlow::Break(Ok(())),
            };

            match recvmsg(
                fd.get_inner().as_raw_fd(),
                data.write_to_iovec(self.data, &mut iovecs),
                &mut state.cmsg_buf,
                MSG_DONTWAIT,
            ) {
                // fd closed on the other side
                Ok((0, ..)) => {
                    trace!(fd = ?fd.get_inner().as_raw_fd(), "closed");

                    ControlFlow::Break(Ok(()))
                }
                Ok((count, ctrl_msg, _flags)) => {
                    b.data.next = match data {
                        Bufs::None => unreachable!(),
                        Bufs::One(range) => {
                            assert!(count <= range.len());
                            range.start + range.len()
                        }
                        Bufs::Two(range1, range2) => {
                            if count < range1.len() {
                                range1.start + count
                            } else {
                                state.buf_state.over_read_data = None;

                                let count = count - range1.len();
                                assert!(count <= range2.len());
                                range2.start + count
                            }
                        }
                    };

                    trace!(
                        fd = fd.get_inner().as_raw_fd(),
                        data_len = count,
                        ctrl_len = ctrl_msg.len(),
                        "received data"
                    );

                    let mut cursor = CmsgCursor::from_ctrl_buf(ctrl_msg);
                    let mut did_read_scm_rights = false;
                    loop {
                        match cursor.read_cmsg() {
                            Some((
                                cmsghdr {
                                    cmsg_type: SOL_SOCKET,
                                    cmsg_level: SCM_RIGHTS,
                                    ..
                                },
                                ctrl_data,
                            )) if !did_read_scm_rights => {
                                did_read_scm_rights = true;
                                let mut fds = ctrl_data.read_as::<RawFd>();
                                b.ctrl.next = match &ctrl {
                                    Bufs::One(range) => {
                                        self.ctrl_range(range)
                                            .start()
                                            .as_ptr()
                                            .copy_from(fds.start(), fds.len());
                                        range.start + fds.len()
                                    }
                                    Bufs::Two(range1, range2) => match fds.split_at(range1.len()) {
                                        None => {
                                            self.ctrl_range(range1)
                                                .start()
                                                .as_ptr()
                                                .copy_from(fds.start(), fds.len());
                                            range1.start + fds.len()
                                        }
                                        Some(first_half) => {
                                            state.buf_state.over_read_ctrl = None;

                                            self.ctrl_range(range1)
                                                .start()
                                                .as_ptr()
                                                .copy_from(first_half.start(), first_half.len());
                                            self.ctrl_range(range2)
                                                .start()
                                                .as_ptr()
                                                .copy_from(fds.start(), fds.len());
                                            range2.start + fds.len()
                                        }
                                    },
                                    Bufs::None => unreachable!(),
                                };
                            }
                            Some((
                                cmsghdr {
                                    cmsg_type: SOL_SOCKET,
                                    cmsg_level: SCM_RIGHTS,
                                    ..
                                },
                                _ctrl_data,
                            )) => {
                                warn!("duplicate SCM_RIGHTS control message");
                            }
                            Some((
                                cmsghdr {
                                    cmsg_type,
                                    cmsg_level,
                                    cmsg_len,
                                },
                                _ctrl_data,
                            )) => {
                                trace!(
                                    fd = fd.get_inner().as_raw_fd(),
                                    cmsg_type,
                                    cmsg_level,
                                    cmsg_len,
                                    "unknown cmsg type, discarding"
                                );
                            }
                            None => {
                                break;
                            }
                        }
                    }

                    ControlFlow::Continue(())
                }
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                    fd.clear_ready_matching(Ready::READABLE);

                    ControlFlow::Break(Ok(()))
                }
                Err(err) => ControlFlow::Break(Err(err)),
            }
        }
    }

    fn parse_message(&self, b: &mut B, state: &mut State) -> ControlFlow<io::Result<()>> {
        unsafe {
            fn get_entry<'a>(
                entry: &mut Option<&'a Entry>,
                map: &'a BTreeMap<new_id, Entry>,
                header: message_header,
            ) -> ControlFlow<(), &'a Entry> {
                match entry {
                    Some(entry) => ControlFlow::Continue(*entry),
                    entry @ None => match map.get(&header.object_id.to_new_id()) {
                        Some(e) => ControlFlow::Continue(*entry.insert(e)),
                        None => {
                            trace!("missing id");
                            ControlFlow::Break(())
                        }
                    },
                }
            }
            let mut entry = None;
            loop {
                match state.parsing_state {
                    ParsingState::None => {
                        let handle = self
                            .try_get_bufs(b, &mut state.buf_state, HDR_LEN, 0)
                            .map_break(Ok)?;

                        let hdr = message_header::read(
                            &mut handle.data.cast_const(),
                            &mut handle.ctrl.cast_const(),
                        )
                        .ok()
                        .unwrap();

                        state.parsing_state = ParsingState::Header(hdr);
                    }
                    ParsingState::Header(header) => {
                        let entry = get_entry(&mut entry, &state.map, header).map_break(Ok)?;

                        let handle = self
                            .try_get_bufs(
                                b,
                                &mut state.buf_state,
                                header.datalen as usize - HDR_LEN,
                                entry.fd_count(header.opcode)?,
                            )
                            .map_break(Ok)?;

                        if Self::DATA_THRESHOLD <= state.buf_state.next_data {
                            state.buf_state.next_data = 0;
                        };

                        if Self::CTRL_THRESHOLD <= state.buf_state.next_ctrl {
                            state.buf_state.next_data = 0;
                        };

                        state.parsing_state = ParsingState::Handle {
                            header,
                            handle,
                            next: NextAlloc {
                                data_next: state.buf_state.next_data,
                                ctrl_next: state.buf_state.next_ctrl,
                            },
                        }
                    }
                    ParsingState::Handle {
                        header,
                        handle,
                        next,
                    } => {
                        let entry = get_entry(&mut entry, &state.map, header).map_break(Ok)?;
                        let slot = self.alloc_slot(&mut b.slot).map_break(Ok)?;

                        {
                            let Ok(mut recv_guard) = entry.recv.try_lock() else {
                                trace!("recvcell is currently being used");
                                return ControlFlow::Break(Ok(()));
                            };

                            if let Err(_handle) = recv_guard.queue.push_back(Handle {
                                slot,
                                hdr: header,

                                inner: handle,
                                next,
                            }) {
                                // dealloc slot
                                b.slot.next = slot;

                                trace!("not enough space in message queue");

                                return ControlFlow::Break(Ok(()));
                            };

                            let waker = recv_guard.waker.take();
                            drop(recv_guard);
                            if let Some(a) = waker {
                                Waker::wake(a)
                            }
                        }

                        state.parsing_state = ParsingState::None;
                        return ControlFlow::Continue(());
                    }
                }
            }
        }
    }
}

const HDR_LEN: usize = 8;

enum Bufs {
    None,
    One(Range<usize>),
    Two(Range<usize>, Range<usize>),
}

impl Bufs {
    fn write_to_iovec<'a>(&self, buf: NonNull<u8>, iovecs: &'a mut [iovec; 2]) -> &'a mut [iovec] {
        fn iovec_from_range(buf: NonNull<u8>, range: &Range<usize>) -> iovec {
            unsafe {
                iovec {
                    iov_base: buf.as_ptr().add(range.start).cast(),
                    iov_len: range.end - range.start,
                }
            }
        }

        match self {
            Bufs::None => &mut [],
            Bufs::One(range) => {
                iovecs[0] = iovec_from_range(buf, range);
                &mut iovecs[..1]
            }
            Bufs::Two(range1, range2) => {
                iovecs[0] = iovec_from_range(buf, range1);
                iovecs[1] = iovec_from_range(buf, range2);
                &mut iovecs[..2]
            }
        }
    }
}

fn slice_with_len<T>(buf: NonNull<T>, offset: usize, end: usize) -> NonNull<[T]> {
    unsafe {
        NonNull::new_unchecked(slice_from_raw_parts_mut(
            buf.add(offset).as_ptr(),
            end - offset,
        ))
    }
}

/// - free is `inclusive`
/// - next is `exclusive`
/// - hold is `exclusive` (short for threshold)
#[inline]
fn free_space(free: usize, next: usize, hold: usize) -> Bufs {
    use std::cmp::Ordering::{Equal, Greater, Less};

    match (free.cmp(&next), free.cmp(&hold), next.cmp(&hold)) {
        //    free <= next && free <  hold && next <  hold
        // => `.. <free..next> .. hold` | next..hold, 0..free
        // => there is still place to write `hold`
        (Less | Equal, Less, Less) => {
            if 0 < free {
                Bufs::Two(next..hold, 0..free - 1)
            } else {
                Bufs::One(next..hold)
            }
        }

        //    free <= next && free < hold && hold <= next
        // => `.. <free.. hold ..next> ..` | 0..free
        // => hold was fulfilled
        (Less | Equal, Less, Equal | Greater) => {
            if 0 < free {
                Bufs::One(0..free - 1)
            } else {
                Bufs::None
            }
        }

        //    free <= next && hold <= free && hold <= next
        // => `..hold .. <free..next> ..`
        // => hold still needs to be fulfilled on the next round
        (Less | Equal, Equal | Greater, Equal | Greater) => {
            if 0 < hold {
                Bufs::One(0..hold)
            } else {
                Bufs::None
            }
        }

        //    free <= next && free >= hold && next < hold
        // => free <= next && hold <= free && next < hold
        // => free <= next && next <  free
        // => impossible
        (Less | Equal, Equal | Greater, Less) => unreachable!("not allowed by math"),

        //    next <  free && free < hold && next < hold
        // => `..next> .. <free .. hold ..` | next..free
        (Greater, Less, Less) => Bufs::One(next..free),

        //    next <  free && hold <= free && next <  hold
        // => `..next> .. hold .. <free..` | next..hold
        (Greater, Equal | Greater, Less) => Bufs::One(next..hold),

        //    next <  free && hold <= free && hold <= next
        // => `..hold .. next> .. <free`
        (Greater, Equal | Greater, Equal | Greater) => Bufs::None,

        //    free >  next && free <  hold && hold <= next
        // => next <  free && free <  hold && hold <= next
        // => next <  hold && hold <  next
        // => impossible
        (Greater, Less, Equal | Greater) => unreachable!("not allowed by math"),
    }
}
