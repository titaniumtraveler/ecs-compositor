use crate::travel_logs::{Buffer, Message};
use std::sync::atomic::Ordering;

struct Bytes;

impl Message for Bytes {
    const MAX_SLOTS: usize = 8;
    type Item = u8;
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

macro_rules! assert_buffer {
    (
    $slot_next:expr,
    $slot_free:expr,
    $data_next:expr,
    $data_free:expr,

    $actual:expr
) => {
        assert_eq!($slot_next, $actual.slot_next.load(Ordering::Acquire));
        assert_eq!($slot_free, $actual.slot_free.load(Ordering::Acquire));

        assert_eq!($data_next, $actual.data_next.load(Ordering::Acquire));
        assert_eq!($data_free, $actual.data_free.load(Ordering::Acquire));
    };
}

#[allow(unused)]
#[test]
fn basic_alloc_dealloc() {
    let buf = Buffer::new(Bytes, 3 + 7 + 5);
    unsafe {
        let buf = &buf;
        assert_buffer!(0, 0, 0, 0, buf);

        let a = buf.allocate(0, 1, 0, 3).unwrap();
        let a_slice = write_slice(buf, 0, [b'a'; 3]);
        assert_buffer!(1, 0, 3, 0, buf);

        let b = buf.allocate(1, 2, 3, 10).unwrap();
        let b_slice = write_slice(buf, 3, [b'b'; 7]);
        assert_buffer!(2, 0, 10, 0, buf);

        let c = buf.allocate(2, 3, 10, 15).unwrap();
        let c_slice = write_slice(buf, 10, [b'c'; 5]);
        assert_buffer!(3, 0, 15, 0, buf);

        assert_eq!([b'a'; 3], a_slice);
        assert_eq!([b'b'; 7], b_slice);
        assert_eq!([b'c'; 5], c_slice);

        buf.dealloc(1, 3);
        assert_buffer!(3, 1, 15, 3, buf);
        buf.dealloc(2, 10);
        assert_buffer!(3, 2, 15, 10, buf);
        buf.dealloc(3, 15);
        assert_buffer!(3, 3, 15, 15, buf);
    }
}
