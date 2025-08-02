use crate::travel_logs::{Buffer, Message, Point, PointRange, Range};

struct Bytes;

unsafe impl Message for Bytes {
    const MAX_SLOTS: usize = 8;
    type Item = u8;

    unsafe fn alloc(&self, _new: PointRange) {
        // TODO: add metadata to fix the TODO in `self.mark_dead()`
    }

    unsafe fn mark_dead(&self, dead: PointRange) -> Option<PointRange> {
        // TODO: Note that this is only okay because the handles are deallocated in the order they
        // were allocated in the first place.
        Some(dead)
    }

    unsafe fn dealloc(
        &self,
        PointRange {
            slot: Range { from: _, to: slot },
            data: Range { from: _, to: data },
        }: PointRange,
    ) -> Point {
        Point { slot, data }
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

#[allow(unused)]
#[test]
fn basic_alloc_dealloc() {
    let buf = Buffer::new(Bytes, 3 + 7 + 5);
    unsafe {
        let buf = &buf;
        assert_eq!(
            PointRange {
                slot: Range { from: 0, to: 0 },
                data: Range { from: 0, to: 0 },
            },
            buf.allocated_range(),
        );

        let a = buf
            .allocate(PointRange {
                slot: Range { from: 0, to: 1 },
                data: Range { from: 0, to: 3 },
            })
            .unwrap();
        let a_slice = write_slice(buf, 0, [b'a'; 3]);
        assert_eq!(
            PointRange {
                slot: Range { from: 0, to: 1 },
                data: Range { from: 0, to: 3 },
            },
            buf.allocated_range()
        );

        let b = buf
            .allocate(PointRange {
                slot: Range { from: 1, to: 2 },
                data: Range { from: 3, to: 10 },
            })
            .unwrap();
        let b_slice = write_slice(buf, 3, [b'b'; 7]);
        assert_eq!(
            PointRange {
                slot: Range { from: 0, to: 2 },
                data: Range { from: 0, to: 10 },
            },
            buf.allocated_range()
        );

        let c = buf
            .allocate(PointRange {
                slot: Range { from: 2, to: 3 },
                data: Range { from: 10, to: 15 },
            })
            .unwrap();
        let c_slice = write_slice(buf, 10, [b'c'; 5]);
        assert_eq!(
            PointRange {
                slot: Range { from: 0, to: 3 },
                data: Range { from: 0, to: 15 }
            },
            buf.allocated_range()
        );

        assert_eq!([b'a'; 3], a_slice);
        assert_eq!([b'b'; 7], b_slice);
        assert_eq!([b'c'; 5], c_slice);

        buf.dealloc(PointRange {
            slot: Range { from: 0, to: 1 },
            data: Range { from: 0, to: 3 },
        });
        assert_eq!(
            PointRange {
                slot: Range { from: 1, to: 3 },
                data: Range { from: 3, to: 15 }
            },
            buf.allocated_range()
        );
        buf.dealloc(PointRange {
            slot: Range { from: 1, to: 2 },
            data: Range { from: 3, to: 10 },
        });
        assert_eq!(
            PointRange {
                slot: Range { from: 2, to: 3 },
                data: Range { from: 10, to: 15 }
            },
            buf.allocated_range(),
        );
        buf.dealloc(PointRange {
            slot: Range { from: 2, to: 3 },
            data: Range { from: 10, to: 15 },
        });
        assert_eq!(
            PointRange {
                slot: Range { from: 3, to: 3 },
                data: Range { from: 15, to: 15 }
            },
            buf.allocated_range()
        );
    }
}
