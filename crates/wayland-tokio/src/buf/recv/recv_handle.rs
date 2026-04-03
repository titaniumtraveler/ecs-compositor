use crate::buf::recv::{Info, Pos, RecvRef, WrappingUsize};
use ecs_compositor_core::{Message, RawSliceExt, Value, message_header};
use phasesync::{CarryingAdd, FreeReturn};
use std::{
    marker::PhantomData,
    ops::{Add, AddAssign, RangeInclusive},
    os::fd::RawFd,
    sync::atomic::Ordering::{Acquire, Relaxed, Release},
};
use tracing::{info, info_span};

#[derive(Debug)]
pub struct RecvHandle {
    pub(crate) buf: RecvRef,
    pub(crate) slot: RangeInclusive<Pos>,
    pub(crate) free: Info,
    pub(crate) span: BufSpan,
}

impl RecvHandle {
    pub fn slot(&self) -> RangeInclusive<Pos> {
        self.slot.clone()
    }

    pub fn span(&self) -> BufSpan {
        self.span
    }

    pub fn data(&self) -> *mut [u8] {
        self.data_slice(self.span.data)
    }

    pub fn ctrl(&self) -> *mut [RawFd] {
        self.ctrl_slice(self.span.ctrl)
    }

    pub fn free(&self) -> Info {
        self.free
    }
}

#[allow(unused_imports)]
use super::RecvState;

#[derive(Debug, Clone, Copy)]
struct Offset {
    data: usize,
    ctrl: usize,
}

#[derive(Debug, Clone, Copy)]
struct Len {
    data: usize,
    ctrl: usize,
}

impl Len {
    const HDR: Self = Self { data: message_header::DATA_LEN as usize, ctrl: message_header::CTRL_LEN };
}

impl Add<Len> for Offset {
    type Output = Self;
    fn add(self, len: Len) -> Self::Output {
        Self { data: self.data + len.data, ctrl: self.ctrl + len.ctrl }
    }
}

impl AddAssign<Len> for Offset {
    fn add_assign(&mut self, len: Len) {
        *self = *self + len
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Buf {
    data: *mut [u8],
    ctrl: *mut [RawFd],
}

impl RecvHandle {
    fn data_len(&self, offset: usize) -> usize {
        let BufSpan { data, data_len, .. } = self.span;
        data_len - (offset - data)
    }
    fn ctrl_len(&self, offset: usize) -> usize {
        let BufSpan { ctrl, ctrl_len, .. } = self.span;
        ctrl_len - (offset - ctrl)
    }

    fn data_slice(&self, offset: usize) -> *mut [u8] {
        self.buf.data_slice(offset, self.data_len(offset))
    }

    fn ctrl_slice(&self, offset: usize) -> *mut [RawFd] {
        self.buf.ctrl_slice(offset, self.ctrl_len(offset))
    }

    fn peek_buf(&self, offset: Offset, len: Len) -> Option<Buf> {
        unsafe {
            if !(self.span.data <= offset.data
                && offset.data + len.data <= self.span.data + self.span.data_len
                && self.span.ctrl <= offset.ctrl
                && offset.ctrl + len.ctrl <= self.span.ctrl + self.span.ctrl_len)
            {
                return None;
            }

            let data = self.data_slice(offset.data).split_at(len.data)?;
            let ctrl = self.ctrl_slice(offset.ctrl).split_at(len.ctrl)?;

            Some(Buf { data, ctrl })
        }
    }
    fn peek_hdr(&self, offset: Offset) -> ecs_compositor_core::primitives::Result<Option<message_header>> {
        unsafe {
            let Some(Buf { data, ctrl }) = self.peek_buf(offset, Len::HDR) else {
                return Ok(None);
            };

            Ok(Some(message_header::read(
                &mut data.cast_const(),
                &mut ctrl.cast_const(),
            )?))
        }
    }
    fn peek_msg(&self, mut offset: Offset, len: Len) -> Option<Buf> {
        offset += Len::HDR;
        let buf = self.peek_buf(offset, len)?;

        Some(buf)
    }
    fn read_msg(
        &self,
        offset: &mut Offset,
        ctrl_len: impl FnOnce(message_header) -> ecs_compositor_core::primitives::Result<usize>,
        advance: bool,
    ) -> ecs_compositor_core::primitives::Result<Option<(message_header, Buf)>> {
        let Some(hdr) = self.peek_hdr(*offset)? else {
            return Ok(None);
        };

        let len = Len { data: hdr.content_len() as _, ctrl: ctrl_len(hdr)? };
        let Some(buf) = self.peek_msg(*offset, len) else {
            return Ok(None);
        };

        if advance {
            *offset += Len::HDR;
            *offset += len;
        }

        Ok(Some((hdr, buf)))
    }

    pub fn cursor(&mut self) -> Cursor<'_> {
        Cursor { offset: Offset { data: self.span.data, ctrl: self.span.ctrl }, handle: self }
    }
}

pub struct Cursor<'handle> {
    handle: &'handle mut RecvHandle,
    offset: Offset,
}

impl<'handle> Cursor<'handle> {
    fn msg(
        &mut self,
        ctrl_len: impl FnOnce(message_header) -> ecs_compositor_core::primitives::Result<usize>,
        advance: bool,
    ) -> ecs_compositor_core::primitives::Result<Option<MsgBuf<'_>>> {
        let Self { handle, offset } = self;
        let Some((header, buf)) = handle.read_msg(offset, ctrl_len, advance)? else {
            return Ok(None);
        };

        Ok(Some(MsgBuf { header, buf, _marker: PhantomData }))
    }

    pub fn peek_msg(
        &mut self,
        ctrl_len: impl FnOnce(message_header) -> ecs_compositor_core::primitives::Result<usize>,
    ) -> ecs_compositor_core::primitives::Result<Option<MsgBuf<'_>>> {
        self.msg(ctrl_len, false)
    }

    pub fn read_msg(
        &mut self,
        ctrl_len: impl FnOnce(message_header) -> ecs_compositor_core::primitives::Result<usize>,
    ) -> ecs_compositor_core::primitives::Result<Option<MsgBuf<'_>>> {
        self.msg(ctrl_len, true)
    }
}

pub struct MsgBuf<'handle> {
    header: message_header,
    buf: Buf,
    _marker: PhantomData<&'handle RecvHandle>,
}

impl MsgBuf<'_> {
    pub fn header(&self) -> message_header {
        self.header
    }
    pub fn msg<'a, M: Message<'a>>(&mut self) -> ecs_compositor_core::primitives::Result<M> {
        unsafe {
            M::read(
                &mut self.buf.data.cast_const(),
                &mut self.buf.ctrl.cast_const(),
            )
        }
    }
}

/// Note:
/// This races with [`RecvState::commit_buf_state()`] when setting `atomic_wait`.
impl Drop for RecvHandle {
    fn drop(&mut self) {
        let _span = info_span!(
            "RecvHandle::drop()",
            slot.start = ?self.slot.start(),
            slot.end = ?self.slot.end(),
            free = ?self.free
        )
        .entered();
        let phase = self.buf.slot_buf();
        let free = &mut self.free;

        let atomic_free = &self.buf.atomic_state().free;

        info!(
            slot.start = ?self.slot.start(),
            slot.end = ?self.slot.end(),
            wait = ?Info(self.buf.atomic_state().wait.load(Relaxed)),
            ?free,
            "dropping handle"
        );

        let atomic_wait = &self.buf.atomic_state().wait;
        let mut wait = atomic_wait.load(Acquire);
        match phase.free_slots(
            self.slot.clone(),
            Info(wait).slot_pos() - WrappingUsize::ONE,
            |new_slot| {
                free.set_slot_pos(new_slot);
                atomic_free.store(free.0, Release);
            },
        ) {
            FreeReturn::Successful => {
                info!("fast_path");
            }
            FreeReturn::Selected { slot: selected } => {
                info!(?selected, ?free, "slow_path");
            }
            FreeReturn::AllSlotsDead => {
                atomic_free.store(free.0, Release);
                'all_slots_dead: loop {
                    info!(?free, "all slots dead");
                    let new_wait = Info(wait).with_all_slots_dead(true).0;
                    match atomic_wait.compare_exchange(wait, new_wait, Release, Acquire) {
                        Ok(_) => {
                            info!("successfully set all slots dead");
                            return;
                        }
                        Err(actual) => {
                            let old_pos = Info(wait).slot_pos();
                            wait = actual;
                            let until = Info(wait).slot_pos();
                            if old_pos != until {
                                match self.buf.slot_buf().set_in_search_range(old_pos..=until, |new_slot| {
                                    free.set_slot_pos(new_slot);
                                    atomic_free.store(free.0, Release);
                                    info!(?free, "commit");
                                }) {
                                    FreeReturn::Successful => {
                                        info!("fast_path");
                                        return;
                                    }
                                    FreeReturn::Selected { slot: selected } => {
                                        info!(?selected, ?free, "slow_path");
                                        return;
                                    }
                                    FreeReturn::AllSlotsDead => {
                                        continue 'all_slots_dead;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BufSpan {
    pub data: usize,
    pub data_len: usize,

    pub ctrl: usize,
    pub ctrl_len: usize,

    pub count: usize,
}

impl BufSpan {
    pub fn new(data: usize, ctrl: usize) -> Self {
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
