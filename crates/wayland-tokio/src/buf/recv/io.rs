use crate::buf::{
    AsyncRecv, IoCount,
    recv::{CTRL_HOLD, ChunkInfo, DATA_HOLD, Info, Pos, RecvHandle, RecvRef, Spans, WrappingUsize},
    span,
};
use ecs_compositor_core::{Value, message_header};
use libc::iovec;
use phasesync::helpers::try_while;
use std::{
    io,
    num::NonZero,
    ops::Range,
    os::fd::RawFd,
    pin::Pin,
    ptr::{null_mut, slice_from_raw_parts_mut},
    sync::atomic::Ordering::{Acquire, Release},
    task::{Context, Poll},
};
use tracing::{info, trace};

#[derive(Debug)]
pub struct RecvState {
    pub recv: Info,

    pub socket_closed: bool,

    pub recv_data_hold: Option<NonZero<u32>>,
    pub recv_ctrl_hold: Option<NonZero<u16>>,

    pub free_data_hold: Option<NonZero<u32>>,
    pub free_ctrl_hold: Option<NonZero<u16>>,
}

impl Default for RecvState {
    fn default() -> Self {
        Self::new()
    }
}

impl RecvState {
    pub fn new() -> Self {
        Self {
            //
            recv: Info::new(),
            socket_closed: false,

            recv_data_hold: None,
            recv_ctrl_hold: None,

            free_data_hold: None,
            free_ctrl_hold: None,
        }
    }

    pub fn recv(
        &mut self,
        sock: Pin<&mut impl AsyncRecv>,
        buf: RecvRef,
        callback: &mut impl Callback,
        cx: &mut Context<'_>,
    ) -> Poll<(Option<RecvHandle>, Option<io::Error>, Option<io::Error>)> {
        let wait = buf.atomic_state().wait.load(Acquire);
        let free = buf.atomic_state().free.load(Acquire);

        let mut bufs = self.init_bufs(wait, free);

        trace! {
            bufs.free.data = ?bufs.free.data,
            bufs.free.ctrl = ?bufs.free.ctrl,
            bufs.recv.data = ?bufs.recv.data,
            bufs.recv.ctrl = ?bufs.recv.ctrl,
            "recv()"
        }

        let io_err = self.read_sock_into_buf(sock, buf, &mut bufs, cx).err();
        trace! {
            err = ?io_err,

            ?self.socket_closed,
            ?bufs.free.data,
            ?bufs.free.ctrl,
            ?bufs.recv.data,
            ?bufs.recv.ctrl,

            ?cx,
            "read_sock_into_buf()"
        }

        let mut handle = BufSpan::new(bufs.recv.data.free, bufs.recv.ctrl.free);
        let parse_err = self.collect_chunks(buf, &mut bufs, &mut handle, callback).err();
        trace! {
            err = ?parse_err,

            ?bufs.free.data,
            ?bufs.free.ctrl,
            ?bufs.recv.data,
            ?bufs.recv.ctrl,

            ?handle,
            "collect_chunks()"
        }

        if handle.count == 0 && !self.socket_closed && io_err.is_none() && parse_err.is_none() {
            self.commit_buf_state(buf, &bufs, None, wait);
            trace! {
                self.socket_closed,
                ?bufs.slots,
                ?bufs.free.data,
                ?bufs.free.ctrl,
                ?bufs.recv.data,
                ?bufs.recv.ctrl,
                ?handle,
                "recv() = Poll::Pending"
            }
            Poll::Pending
        } else {
            let handle = self.alloc_handle(buf, &mut bufs, handle, callback);
            self.commit_buf_state(buf, &bufs, handle.as_ref(), wait);
            trace! {
                self.socket_closed,
                ?bufs.free.data,
                ?bufs.free.ctrl,
                ?bufs.recv.data,
                ?bufs.recv.ctrl,
                handle = ?handle,
                io_err = ?io_err,
                parse_err = ?parse_err,
                "recv() = Poll::Ready()"
            }
            Poll::Ready((handle, io_err, parse_err))
        }
    }

    fn read_sock_into_buf(
        &mut self,
        mut sock: Pin<&mut impl AsyncRecv>,
        buf: RecvRef,

        bufs: &mut Bufs,

        cx: &mut Context<'_>,
    ) -> io::Result<()> {
        if self.socket_closed {
            return Ok(());
        }

        let mut data_buf = [iovec { iov_base: null_mut(), iov_len: 0 }; 2];
        let mut ctrl_buf = [iovec { iov_base: null_mut(), iov_len: 0 }; 2];

        loop {
            let IoCount { data, ctrl } = match sock.as_mut().poll_recv(
                &mut bufs.free.data.free_space(buf.data(), &mut data_buf),
                &mut bufs.free.ctrl.free_space(buf.ctrl(), &mut ctrl_buf),
                cx,
            ) {
                Poll::Ready(Ok(IoCount { data: 0, ctrl: 0 })) => {
                    trace!("read_sock_into_buf() = (0,0)");
                    self.socket_closed = true;
                    break;
                }
                Poll::Ready(Ok(count)) => count,
                Poll::Ready(Err(err)) => return Err(err),
                Poll::Pending => return Ok(()),
            };

            bufs.recv.data.produce(data);
            bufs.recv.ctrl.produce(ctrl);

            bufs.free.data.consume(data);
            bufs.free.ctrl.consume(ctrl);
        }

        Ok(())
    }

    fn collect_chunks(
        &mut self,
        buf: RecvRef,
        bufs: &mut Bufs,

        handle: &mut BufSpan,
        callback: &mut impl Callback,
    ) -> io::Result<()> {
        loop {
            {
                let BufSpan { data, data_len, ctrl, ctrl_len, .. } = *handle;
                if !(data + data_len == bufs.recv.data.free && ctrl + ctrl_len == bufs.recv.ctrl.free) {
                    // handle would not be contiguos
                    return Ok(());
                }
            }

            let hdr = {
                let Some((data, ctrl)) = bufs.read_slices(buf, message_header::DATA_LEN as usize, 0, false) else {
                    trace!("waiting on message header");
                    return Ok(());
                };
                unsafe { message_header::read(&mut data.cast_const(), &mut ctrl.cast_const())? }
            };

            if !callback.should_include(hdr) {
                trace!("stopped by callback");
                return Ok(());
            }

            let data_len = hdr.datalen as usize;
            let ctrl_len = callback.fd_count(hdr);

            let Some(_) = bufs.read_slices(buf, data_len, ctrl_len, true) else {
                trace!("waiting to message body");
                return Ok(());
            };

            bufs.recv.data.consume(data_len);
            bufs.recv.ctrl.consume(ctrl_len);

            handle.data_len += data_len;
            handle.ctrl_len += ctrl_len;
            handle.count += 1;
        }
    }

    fn init_bufs(&self, wait: u64, free: u64) -> Bufs {
        let Self { recv, recv_data_hold, recv_ctrl_hold, free_data_hold, free_ctrl_hold, .. } = *self;

        info!(wait = ?Info(wait), free = ?Info(free), "RecvState::init_bufs()");
        Bufs {
            slots: Info(wait).slot_pos()..Info(free).slot_pos(),
            free_next: Info(free),
            free: {
                let mut free = Info(free);
                if recv_data_hold.is_none() && free.data() <= recv.data() {
                    free.set_data(DATA_HOLD);
                }
                if recv_ctrl_hold.is_none() && free.ctrl() <= recv.ctrl() {
                    free.set_ctrl(CTRL_HOLD);
                }

                Spans::new(recv, free, recv_data_hold, recv_ctrl_hold)
            },
            recv: Spans::new(Info(wait), recv, free_data_hold, free_ctrl_hold),
        }
    }

    /// Note:
    /// This function races with [`RecvHandle`]'s `Drop` implementation when setting `atomic_wait`.
    pub(super) fn commit_buf_state(&mut self, buf: RecvRef, bufs: &Bufs, handle: Option<&RecvHandle>, wait: u64) {
        self.recv
            .with_data(bufs.recv.data.next.try_into().unwrap())
            .with_ctrl(bufs.recv.ctrl.next.try_into().unwrap());

        let atomic_wait = &buf.atomic_state().wait;
        let mut wait = wait;
        loop {
            let mut new_wait = Info(wait);
            new_wait
                .with_data(bufs.recv.data.free.try_into().unwrap())
                .with_ctrl(bufs.recv.ctrl.free.try_into().unwrap())
                .with_slot_pos(bufs.slots.start);

            if let Some(handle) = handle
                && new_wait.all_slots_dead()
            {
                let slot = *handle.slot.start();
                let chunk =
                    buf.slot_buf()
                        .load_chunk(ChunkInfo { chunk: slot.chunk, lower: slot.index, upper: slot.index });
                try_while(chunk.chunk, chunk.val, |_| true, |val| val & !chunk.mask);
                new_wait.set_all_slots_dead(false);
            }

            // trace!(?new_wait, old_wait = ?Info(wait), "buf.wait = new_wait");
            info!(old_wait = ?Info(wait), wait = ?Info(wait), ?new_wait, "buf.wait = new_wait");
            match atomic_wait.compare_exchange(wait, new_wait.0, Release, Acquire) {
                Ok(_) => break,
                Err(actual) => wait = actual,
            }
        }
    }

    fn alloc_handle(
        &mut self,
        buf: RecvRef,
        bufs: &mut Bufs,
        handle: BufSpan,
        callback: &mut impl Callback,
    ) -> Option<RecvHandle> {
        info!(?bufs.slots.start, ?bufs.slots.end, "RecvState::alloc_handle()");
        if handle.data_len == 0 && handle.ctrl_len == 0 {
            return None;
        }

        let slot_count = callback.slot_count(handle.count.max(1));
        callback.finish();
        let slot = {
            let Range { start, end } = &mut bufs.slots;
            let allocated_slots = *start + WrappingUsize::new(slot_count - 1);
            info!(?start, ?end, ?allocated_slots, slot_count, "slots");

            let is_in_bounds = if *start < *end {
                *start <= allocated_slots && allocated_slots < *end
            } else {
                *start <= allocated_slots || allocated_slots < *end
            };

            if !is_in_bounds {
                todo!("Handle case where not enough slots are available")
            }

            let old_start = *start;
            *start += WrappingUsize::new(slot_count);

            old_start..=allocated_slots
        };

        let data = buf.data_slice(handle.data, handle.data_len);
        let ctrl = buf.ctrl_slice(handle.ctrl, handle.ctrl_len);

        let free = {
            let BufSpan { data, data_len, ctrl, ctrl_len, count: _ } = handle;
            *Info::new()
                .with_data((data + data_len) as u32)
                .with_ctrl((ctrl + ctrl_len) as u16)
                .with_slot_pos(*slot.end() + WrappingUsize::new(1))
        };

        Some(RecvHandle { slot, buf, data, ctrl, free })
    }
}

#[derive(Debug, Clone, Copy)]
struct BufSpan {
    data: usize,
    data_len: usize,

    ctrl: usize,
    ctrl_len: usize,

    count: usize,
}

impl BufSpan {
    fn new(data: usize, ctrl: usize) -> Self {
        Self {
            //
            data,
            data_len: 0,

            ctrl,
            ctrl_len: 0,

            count: 0,
        }
    }
}

#[derive(Debug)]
pub(super) struct Bufs {
    slots: Range<Pos>,
    free_next: Info,

    free: Spans,
    recv: Spans,
}

impl Bufs {
    fn read_slices(
        &mut self,
        buf: RecvRef,
        data_len: usize,
        ctrl_len: usize,
        allow_split: bool,
    ) -> Option<(*mut [u8], *mut [RawFd])> {
        unsafe {
            let data = 'data: {
                if data_len == 0 {
                    break 'data slice_from_raw_parts_mut(null_mut(), 0);
                }

                match self.recv.data.get_ranges() {
                    span::Bufs::None => return None,
                    span::Bufs::One(range) | span::Bufs::Two(range, _) => {
                        let Range { start, end } = range;
                        if (end - start) < data_len {
                            let hold = start + data_len;
                            if (DATA_HOLD as usize) < hold {
                                self.recv.data.next = hold;
                                match allow_split {
                                    false => {
                                        debug_assert!(
                                            self.free.data.hold == 0,
                                            "hold should always be 0 at this point!"
                                        );
                                        self.free.data.next = hold;
                                    }
                                    true => {
                                        self.free.data.hold = hold;
                                        self.free.data.next = self.free_next.data() as usize;
                                    }
                                }
                            }

                            return None;
                        }

                        slice_from_raw_parts_mut(buf.data().add(start), data_len)
                    }
                }
            };
            let ctrl = 'ctrl: {
                if ctrl_len == 0 {
                    break 'ctrl slice_from_raw_parts_mut(null_mut(), 0);
                }
                match self.recv.ctrl.get_ranges() {
                    span::Bufs::None => return None,
                    span::Bufs::One(range) | span::Bufs::Two(range, _) => {
                        let Range { start, end } = range;
                        let hold = start + ctrl_len;
                        if (end - start) < ctrl_len {
                            if (CTRL_HOLD as usize) < hold {
                                self.recv.ctrl.hold = hold;

                                match allow_split {
                                    false => {
                                        debug_assert!(
                                            self.free.ctrl.hold == 0,
                                            "hold should always be 0 at this point!"
                                        );
                                        self.free.ctrl.next = hold;
                                    }
                                    true => {
                                        self.free.ctrl.hold = hold;
                                        self.free.ctrl.next = self.free_next.ctrl() as usize;
                                    }
                                }
                            }

                            return None;
                        }

                        slice_from_raw_parts_mut(buf.ctrl().add(start), ctrl_len)
                    }
                }
            };

            Some((data, ctrl))
        }
    }
}

pub trait Callback {
    fn fd_count(&mut self, hdr: message_header) -> usize;
    fn should_include(&mut self, hdr: message_header) -> bool;
    fn slot_count(&mut self, count: usize) -> usize;
    fn finish(&mut self) {}
}
