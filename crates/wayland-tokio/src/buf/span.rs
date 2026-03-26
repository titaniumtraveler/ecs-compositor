use libc::iovec;
use std::{fmt::Debug, marker::PhantomData, ops::Range, ptr::slice_from_raw_parts_mut};
use tracing::instrument;

#[derive(Default)]
pub struct Span<const DEFAULT_HOLD: usize, IndexUnit> {
    pub free: usize,
    pub next: usize,
    pub hold: usize,

    pub _marker: PhantomData<IndexUnit>,
}

impl<const DEFAULT_HOLD: usize, Unit> Debug for Span<DEFAULT_HOLD, Unit> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Span")
            .field("free", &self.free)
            .field("next", &self.next)
            .field("hold", &self.hold)
            .field("_marker", &self._marker)
            .finish()
    }
}

impl<const DEFAULT_HOLD: usize, Unit> Span<DEFAULT_HOLD, Unit> {
    pub fn free_space<'a>(&self, buf: *mut Unit, iovecs: &'a mut [iovec; 2]) -> &'a mut [iovec] {
        fn iovec_from_range<Unit>(buf: *mut Unit, range: Range<usize>) -> iovec {
            unsafe {
                iovec { iov_base: buf.add(range.start).cast(), iov_len: (range.end - range.start) * size_of::<Unit>() }
            }
        }

        let Self { free, next, hold, .. } = *self;
        let bufs = free_space(free, next, hold);

        match bufs {
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

    #[instrument(ret)]
    /// # Safety
    /// TODO
    pub unsafe fn read_slice(&mut self, buf: *mut Unit, len: usize) -> Option<*mut [Unit]> {
        let bufs = free_space(self.free, self.next, self.hold);
        match bufs {
            Bufs::None => None,
            Bufs::One(range) | Bufs::Two(range, _) => {
                let Range { start, end } = range;
                if (end - start) < len {
                    return None;
                }

                unsafe { Some(slice_from_raw_parts_mut(buf.add(start), len)) }
            }
        }
    }

    pub fn get_ranges(&self) -> Bufs {
        free_space(self.free, self.next, self.hold)
    }

    pub fn consume(&mut self, count: usize) {
        let Self { free, hold, .. } = self;
        *free += count;
        if free == hold {
            *free = 0;
            *hold = 0;
        }
    }

    pub fn produce(&mut self, count: usize) {
        let Self { next, hold, .. } = self;
        *next += count;
        if *hold != 0 && hold <= next {
            *next -= *hold;
        }
    }
}

#[derive(Debug)]
pub enum Bufs {
    None,
    One(Range<usize>),
    Two(Range<usize>, Range<usize>),
}

/// - free is `inclusive`
/// - next is `exclusive`
/// - hold is `exclusive` (short for threshold)
#[inline]
#[instrument(level = "trace", ret)]
fn free_space(free: usize, next: usize, hold: usize) -> Bufs {
    match free <= next {
        true if free == next => Bufs::None,
        true => Bufs::One(free..next),
        false => Bufs::Two(free..hold, 0..next),
    }
}
