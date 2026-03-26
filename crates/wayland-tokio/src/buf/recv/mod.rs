use crate::buf::{macros::gen_bitfield, span::Span};
use bitfield::{bitfield, bitfield_fields};
use ecs_compositor_core::message_header;
use phasesync::{CarryingAdd, FreeReturn, WrappingU6};
use std::{
    fmt::Debug,
    num::NonZero,
    ops::RangeInclusive,
    os::fd::RawFd,
    ptr::slice_from_raw_parts_mut,
    sync::atomic::{
        AtomicU64,
        Ordering::{Acquire, Relaxed, Release},
    },
};
use tracing::{info, info_span};

pub use self::io::RecvState;

const SLOT_CHUNK_MAX: usize = SLOT_CHUNK_LEN - 1;
const SLOT_CHUNK_LEN: usize = 1 << Info::FIELDS.slot_chunk.len as usize;

type Phasesync = phasesync::Phasesync<SLOT_CHUNK_MAX, SLOT_CHUNK_LEN>;
type Pos = phasesync::Pos<SLOT_CHUNK_MAX>;
type WrappingUsize = phasesync::WrappingUsize<SLOT_CHUNK_MAX>;

type ChunkInfo = phasesync::chunk_iter::ChunkInfo<SLOT_CHUNK_MAX>;

pub mod io;

pub struct RecvBuf {
    slot_buf: Phasesync,
    data_buf: [u32; (1 << Info::FIELDS.data.len as usize) / size_of::<u32>()],
    ctrl_buf: [RawFd; 1 << Info::FIELDS.ctrl.len as usize],

    atomic_state: AtomicState,
}

impl Default for RecvBuf {
    fn default() -> Self {
        Self::new()
    }
}

impl RecvBuf {
    pub fn new() -> Self {
        Self {
            slot_buf: Phasesync::new(),
            data_buf: [0; _],
            ctrl_buf: [0; _],
            atomic_state: AtomicState {
                free: AtomicInfo::new(Info(0).0),
                wait: AtomicInfo::new(Info(0).with_all_slots_dead(true).0),
            },
        }
    }
}

/// # Invariant
/// It guaranteed that `*mut RecvBuf` is always valid.
#[derive(Debug, Clone, Copy)]
pub struct RecvRef(*mut RecvBuf);

pub struct AtomicState {
    pub free: AtomicInfo,
    pub wait: AtomicInfo,
}

impl Debug for AtomicState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AtomicState")
            .field("free", &Info(self.free.load(Relaxed)))
            .field("wait", &Info(self.wait.load(Relaxed)))
            .finish()
    }
}

fn buf_slice<T>(buf: *mut T, offset: usize, len: usize) -> *mut [T] {
    unsafe { slice_from_raw_parts_mut(buf.add(offset), len) }
}

impl RecvRef {
    /// # Safety
    /// Caller has to guarantee that the [`RecvBuf`] is valid and all derived [`RecvHandle`] are
    /// dropped **before** its resources are released!
    ///
    /// Read as: All [`RecvHandle`]s contain a reference to [`RecvBuf`], so dropping it before the
    /// handles is instant undefined behavior!
    pub unsafe fn new(buf: *mut RecvBuf) -> Self {
        Self(buf)
    }

    pub fn atomic_state<'a>(self) -> &'a AtomicState {
        unsafe { &(*self.0).atomic_state }
    }

    fn slot_buf<'a>(self) -> &'a Phasesync {
        unsafe { &(*self.0).slot_buf }
    }

    fn data(self) -> *mut u8 {
        unsafe { &raw mut (*self.0).data_buf as _ }
    }

    fn data_slice(&self, data: usize, data_len: usize) -> *mut [u8] {
        buf_slice(self.data(), data, data_len)
    }

    fn ctrl(self) -> *mut RawFd {
        unsafe { &raw mut (*self.0).ctrl_buf as _ }
    }

    fn ctrl_slice(&self, ctrl: usize, ctrl_len: usize) -> *mut [i32] {
        buf_slice(self.ctrl(), ctrl, ctrl_len)
    }
}

type AtomicInfo = AtomicU64;

bitfield! {
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub struct Info(u64);
    impl new;
}

gen_bitfield! {
    struct Info

    {
        let wayland_min_len = message_header::DATA_LEN as usize;
        let wayland_max_len = 1 << 16;

        let data_buf_len = wayland_max_len * 4;
    }

    pub u32, slot_chunk, set_slot_chunk, with_slot_chunk = data_buf_len / wayland_min_len / 64;
    pub u8,  slot_index, set_slot_index, with_slot_index = 64;

    pub u32, data,       set_data,       with_data       = data_buf_len;
    pub u16, ctrl,       set_ctrl,       with_ctrl       = 1024;
}

impl ::std::fmt::Debug for Info {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.debug_struct("Info") //
            .field("slot_chunk", &self.slot_chunk())
            .field("slot_index", &self.slot_index())
            .field("data", &self.data())
            .field("ctrl", &self.ctrl())
            .field("all_slots_dead", &self.all_slots_dead())
            .finish()
    }
}

impl Info {
    pub fn slot_pos(&self) -> Pos {
        Pos {
            //
            chunk: WrappingUsize::new(self.slot_chunk() as usize),
            index: WrappingU6::new(self.slot_index()),
        }
    }
    pub fn set_slot_pos(&mut self, pos: Pos) {
        self.set_slot_chunk(*pos.chunk as u32);
        self.set_slot_index(*pos.index);
    }
    pub fn with_slot_pos(&mut self, pos: Pos) -> &mut Self {
        self.set_slot_pos(pos);
        self
    }

    bitfield_fields! {
        pub u8, all_slots_dead, set_all_slots_dead: Info::FIELDS.ctrl.msb as usize + 1;
    }

    fn with_all_slots_dead(&mut self, value: bool) -> &mut Self {
        self.set_all_slots_dead(value);
        self
    }
}

const DATA_HOLD: u32 = (1 << Info::FIELDS.data.len) - (1 << 16);
const CTRL_HOLD: u16 = (1 << Info::FIELDS.ctrl.len) - 16;

#[derive(Debug)]
struct Spans {
    data: Span<{ DATA_HOLD as usize }, u8>,
    ctrl: Span<{ CTRL_HOLD as usize }, RawFd>,
}

impl Spans {
    pub fn new(free: Info, next: Info, data_hold: Option<NonZero<u32>>, ctrl_hold: Option<NonZero<u16>>) -> Self {
        Self {
            data: Span {
                free: free.data() as usize,
                next: next.data() as usize,
                hold: data_hold.map(NonZero::get).unwrap_or(0) as usize,
                ..Default::default()
            },
            ctrl: Span {
                free: free.ctrl() as usize,
                next: next.ctrl() as usize,
                hold: ctrl_hold.map(NonZero::get).unwrap_or(0) as usize,
                ..Default::default()
            },
        }
    }
}

#[test]
fn list_consts() {
    println!("{:#?}", Info::FIELDS);
    println!("{:#X}", size_of::<usize>());
    println!("{:#X}", size_of::<RecvBuf>());

    println!(
        "page count: {0:}, {0:#x}",
        size_of::<RecvBuf>().div_ceil(0x1000)
    );

    println!("fields: {:?}", Info::FIELDS);
}

#[derive(Debug)]
pub struct RecvHandle {
    pub buf: RecvRef,
    pub slot: RangeInclusive<Pos>,
    pub free: Info,
    pub data: *mut [u8],
    pub ctrl: *mut [RawFd],
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
