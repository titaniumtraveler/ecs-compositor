use crate::{WaylandPos, bitfield::BitField};
use std::{
    num::NonZero,
    ops::{
        Bound::{self, *},
        RangeBounds,
    },
    os::fd::RawFd,
    ptr::NonNull,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering::*},
    },
};

pub struct Buffer {
    slot: NonNull<[AtomicU64; slot::UPPER_CAP as usize]>,
    data: NonNull<[u8; data::CAP as usize]>,
    ctrl: NonNull<[RawFd; ctrl::CAP as usize]>,

    free: AtomicU64,
    next: AtomicU64,

    reader_state: Mutex<State>,
}

impl Buffer {
    fn slot_chunk(&self, index: u16) -> &AtomicU64 {
        debug_assert!(index <= slot::UPPER_CAP);
        unsafe { self.slot.cast::<AtomicU64>().add(index.into()).as_ref() }
    }
}

struct State {
    data: Range<u32>,
    ctrl: Range<u16>,
}

#[allow(non_camel_case_types)]
#[derive(Clone, Copy)]
struct slot(u16);
impl BitField<15, u16> for slot {}

#[allow(non_camel_case_types)]
#[derive(Clone, Copy)]
struct data(u32);
impl BitField<18, u32> for data {}

#[allow(non_camel_case_types)]
#[derive(Clone, Copy)]
struct ctrl(u16);
impl BitField<10, u16> for ctrl {}

impl slot {
    const UPPER_CAP: u16 = 1 << (Self::WIDTH - 6);

    const fn new(upper: u16, lower: u16) -> Self {
        let lower_mask = (1 << 6) - 1;
        let upper_mask = (1 << (Self::WIDTH - 6)) - 1;
        slot(((upper & upper_mask) << 6) | (lower & lower_mask))
    }

    const fn get(self) -> (u16, u32) {
        (self.upper(), self.lower() as u32)
    }

    // index into the u64 atomic
    const fn lower(self) -> u16 {
        self.0 & ((1 << 6) - 1)
    }
    // index into the u64 array
    const fn upper(self) -> u16 {
        self.0 >> 6
    }
}

pub struct Handle {
    slot: slot,
    data: Range<data>,
    ctrl: Range<ctrl>,
}

struct Range<T> {
    next: T,
    free: T,
}

impl Handle {
    fn next(&self) -> WaylandPos {
        WaylandPos { slot: self.slot.0, data: self.data.next.0, ctrl: self.ctrl.next.0 }
    }
}

const fn find_first_one(val: u64) -> Option<u32> {
    let Some(val) = NonZero::new(val) else {
        return None;
    };

    Some(u64::BITS - 1 - val.leading_zeros())
}

/// Calculates `(1 << end) - (1 << start)` while also handling all the possible edge_cases.
fn bit_mask_range(bound: impl RangeBounds<u32>) -> u64 {
    const fn inner((start_bound, end_bound): (Bound<u32>, Bound<u32>)) -> u64 {
        let lower = match start_bound {
            Bound::Included(val) => val,
            Bound::Excluded(val) => val + 1,
            Bound::Unbounded => 0,
        };

        let upper = match end_bound {
            Bound::Excluded(val) => val,
            Bound::Included(val) => val + 1,
            Bound::Unbounded => 64,
        };

        match (lower, upper) {
            (l, u) if u <= l => 0,
            (64.., _) => 0,
            (l, 64..) => u64::MAX - ((1 << l) - 1),
            (l, u) => (1 << u) - (1 << l),
        }
    }

    inner((
        bound.start_bound().map(|val| *val),
        bound.end_bound().map(|val| *val),
    ))
}

impl Buffer {
    pub fn alloc_handle(&self) -> Handle {
        todo!()
    }

    pub fn free_handle(&self, handle: Handle) {
        let (upper, lower) = handle.slot.get();
        let mut chunk = self.slot_chunk(upper);

        let mask = 1 << lower;

        let mut val = chunk.load(Acquire);
        loop {
            if (val & mask) == 0 {
                // The handles bit was set to 0 from the outside and is now responsible
                // for freeing the section.

                self.free.store(handle.next().into_64(), Release);
                loop_until_success(chunk, &mut val, |val| val | mask, |_| true);

                break;
            }

            match chunk.compare_exchange(val, val & !mask, Release, Acquire) {
                Ok(_) => {
                    // Handle was freed sucessfully.
                    // No further action is required from us
                    return;
                }
                Err(v) => {
                    val = v;
                    continue;
                }
            }
        }

        if self.handle_chunk(chunk, val, (Excluded(lower), Unbounded)) {
            return;
        }
        let mut chunk_index = upper;

        loop {
            assign_add_wrap::<{ slot::UPPER_CAP }>(&mut chunk_index, 1);
            chunk = self.slot_chunk(chunk_index);
            val = chunk.load(Acquire);

            if chunk_index == upper {
                if self.handle_chunk(chunk, val, ..lower) {
                    return;
                } else {
                    // Not yet sure how to handle this case
                    todo!("buffer was completely full and is now empty again")
                }
            }

            if self.handle_chunk(chunk, val, ..) {
                return;
            }
        }
    }
}

fn loop_until_success(
    chunk: &AtomicU64,
    val: &mut u64,
    mut f: impl FnMut(u64) -> u64,
    mut should_continue: impl FnMut(u64) -> bool,
) -> bool {
    loop {
        let Err(actual) = chunk.compare_exchange(*val, f(*val), Release, Acquire) else {
            break true;
        };
        *val = actual;

        if !should_continue(*val) {
            break false;
        }
    }
}

impl Buffer {
    fn handle_chunk(&self, chunk: &AtomicU64, mut val: u64, range: impl RangeBounds<u32>) -> bool {
        let (start, end) = (
            range.start_bound().map(|b| *b),
            range.end_bound().map(|b| *b),
        );

        loop {
            match find_first_one(val & bit_mask_range((start, end))) {
                Some(first_one) => {
                    let range = (start, Excluded(first_one));
                    let was_success = loop_until_success(
                        chunk,
                        &mut val,
                        |val| val | bit_mask_range(range) & !(1 << first_one),
                        |val| val & (1 << first_one) == 1,
                    );

                    if was_success {
                        break true;
                    }
                }
                None => {
                    loop_until_success(
                        chunk,
                        &mut val,
                        |val| val | bit_mask_range((start, end)),
                        |_| true,
                    );
                    break false;
                }
            }
        }
    }
}

const fn assign_add_wrap<const WRAP: u16>(s: &mut u16, add: u16) {
    debug_assert!(*s < WRAP);
    let diff = WRAP - *s;
    if add < diff {
        *s += add;
    } else {
        *s = add - diff;
    }
}

#[test]
fn test_assing_add_wrap_normal_case() {
    let mut s = 16;
    assign_add_wrap::<32>(&mut s, 15);
    assert_eq!(s, 31);
}

#[test]
fn test_assing_add_wrap_wrapping_case() {
    let mut s = 16;
    assign_add_wrap::<32>(&mut s, 17);
    assert_eq!(s, 1);
}
