use crate::travel_logs::{Buffer, Handle, Message, Point, PointRange, Range};
use bitvec::{array::BitArray, slice::BitSlice};
use std::{
    sync::{Arc, atomic::AtomicU8},
    thread::sleep,
    time::Duration,
};

#[derive(Debug)]
struct Bytes {
    slots: BitArray<[AtomicU8; 1]>,
}

fn find_alive_mark_dead(
    slice: &BitSlice<AtomicU8>,
    range: std::ops::Range<usize>,
) -> Option<usize> {
    if let Some(idx) = slice[range.clone()].iter_ones().next() {
        let idx = range.start + idx;
        for idx in range.start..idx {
            slice.set_aliased(idx, true);
        }

        Some(idx)
    } else {
        for idx in range {
            slice.set_aliased(idx, true);
        }
        None
    }
}

unsafe impl Message for Bytes {
    const MAX_SLOTS: usize = 8;
    type Item = u8;

    unsafe fn alloc(&self, _new: PointRange) {}

    unsafe fn mark_dead(&self, allocated: PointRange, dead: PointRange) -> Option<PointRange> {
        if allocated.slot.from == dead.slot.from {
            let slice = self.slots.as_bitslice();

            let ranges = Range {
                from: dead.slot.upto,
                upto: allocated.slot.upto,
            }
            .into_ring_bounds(Self::MAX_SLOTS);

            if let Some(idx) = find_alive_mark_dead(slice, ranges.0) {
                Some(PointRange {
                    slot: Range {
                        from: allocated.slot.from,
                        upto: idx,
                    },
                    data: dead.data,
                })
            } else if let Some(range) = ranges.1 {
                match find_alive_mark_dead(slice, range) {
                    Some(idx) => Some(PointRange {
                        slot: Range {
                            from: allocated.slot.from,
                            upto: idx,
                        },
                        data: dead.data,
                    }),
                    None => Some(allocated),
                }
            } else {
                Some(allocated)
            }
        } else {
            Some(PointRange {
                slot: allocated.slot,
                data: allocated.data,
            })
        }
    }

    unsafe fn dealloc(&self, range: PointRange) -> Point {
        range.to()
    }
}

impl Bytes {
    pub fn new() -> Self {
        Self {
            slots: BitArray::new([AtomicU8::new(0xFF)]),
        }
    }
}

fn select_contiguos_range(
    ranges: (std::ops::Range<usize>, Option<std::ops::Range<usize>>),
    len: usize,
) -> Option<Range> {
    if len <= ranges.0.clone().count() {
        Some(Range {
            from: ranges.0.start,
            upto: ranges.0.start + len,
        })
    } else if let Some(range) = ranges.1 {
        if len <= range.into_iter().count() {
            Some(Range {
                from: ranges.0.start,
                upto: len,
            })
        } else {
            None
        }
    } else {
        None
    }
}

impl Buffer<Bytes> {
    fn alloc_n(&self, bytes: usize) -> Option<Handle<'_, Bytes>> {
        loop {
            let PointRange { slot, data } = self.allocated_range();
            let slot = select_contiguos_range(
                slot.invert(self.buf.len())
                    .into_ring_bounds(Bytes::MAX_SLOTS),
                1,
            )?;
            let data = select_contiguos_range(
                data.invert(self.buf.len()).into_ring_bounds(self.buf.len()),
                bytes,
            )?;

            match unsafe { self.allocate(PointRange { slot, data }) } {
                None => continue,
                Some(handle) => break Some(handle),
            }
        }
    }
}

fn raw_sub_slice<T>(ptr: *mut [T], idx: usize, len: usize) -> *mut [T] {
    unsafe { std::ptr::slice_from_raw_parts_mut((ptr as *mut T).add(idx), len) }
}

fn byte_set<const LEN: usize, T>(slice: *mut [T], vals: [T; LEN]) {
    unsafe {
        assert_eq!(slice.len(), LEN);
        std::ptr::write(slice as *mut [T; LEN], vals);
    }
}

fn write_slice<const LEN: usize, T: Message>(
    buf: &Buffer<T>,
    idx: usize,
    init: [T::Item; LEN],
) -> &[T::Item] {
    let slice = raw_sub_slice(buf.buf, idx, LEN);
    byte_set(slice, init);
    unsafe { &*slice }
}

#[test]
fn basic_test() {
    let buf = Buffer::new(Bytes::new(), 3 + 7 + 5 + 1);
    let buf = &buf;
    assert_eq!(
        PointRange {
            slot: Range { from: 0, upto: 0 },
            data: Range { from: 0, upto: 0 },
        },
        buf.allocated_range(),
    );

    let a = buf.alloc_n(3).unwrap();
    let a_slice = write_slice(buf, 0, [b'a'; 3]);
    assert_eq!(
        PointRange {
            slot: Range { from: 0, upto: 1 },
            data: Range { from: 0, upto: 3 },
        },
        buf.allocated_range()
    );

    let b = buf.alloc_n(7).unwrap();
    let b_slice = write_slice(buf, 3, [b'b'; 7]);
    assert_eq!(
        PointRange {
            slot: Range { from: 0, upto: 2 },
            data: Range { from: 0, upto: 10 },
        },
        buf.allocated_range()
    );

    let c = buf.alloc_n(5).unwrap();
    let c_slice = write_slice(buf, 10, [b'c'; 5]);
    assert_eq!(
        PointRange {
            slot: Range { from: 0, upto: 3 },
            data: Range { from: 0, upto: 15 }
        },
        buf.allocated_range()
    );

    assert_eq!([b'a'; 3], a_slice);
    assert_eq!([b'b'; 7], b_slice);
    assert_eq!([b'c'; 5], c_slice);

    drop(a);
    assert_eq!(
        PointRange {
            slot: Range { from: 1, upto: 3 },
            data: Range { from: 3, upto: 15 }
        },
        buf.allocated_range()
    );
    drop(b);
    assert_eq!(
        PointRange {
            slot: Range { from: 2, upto: 3 },
            data: Range { from: 10, upto: 15 }
        },
        buf.allocated_range(),
    );
    drop(c);
    assert_eq!(
        PointRange {
            slot: Range { from: 3, upto: 3 },
            data: Range { from: 15, upto: 15 }
        },
        buf.allocated_range()
    );
}

#[test]
fn out_of_order() {
    let buf = Arc::new(Buffer::new(Bytes::new(), 3 + 7 + 5 + 1));

    let a = std::thread::spawn({
        let buf = buf.clone();
        move || {
            let a = buf.alloc_n(3).unwrap();
            sleep(Duration::from_secs(1));
            drop(a);
        }
    });
    let b = std::thread::spawn({
        let buf = buf.clone();
        move || {
            let b = buf.alloc_n(7).unwrap();
            sleep(Duration::from_secs(1));
            drop(b);
        }
    });
    let c = std::thread::spawn({
        let buf = buf.clone();
        move || {
            let c = buf.alloc_n(5).unwrap();
            sleep(Duration::from_secs(1));
            drop(c);
        }
    });

    a.join().unwrap();
    b.join().unwrap();
    c.join().unwrap();
}
