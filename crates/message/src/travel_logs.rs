#![allow(dead_code)]

use std::{
    alloc::Layout,
    sync::atomic::{AtomicUsize, Ordering},
};

#[cfg(test)]
mod tests;

struct Buffer<T: Message> {
    buf: *mut [T::Item],
    data: T,

    slot_next: AtomicUsize,
    slot_free: AtomicUsize,

    data_next: AtomicUsize,
    data_free: AtomicUsize,
}

/// # Safety
/// Implementing this incorrectly will cause undefined behavior
unsafe trait Message {
    const MAX_SLOTS: usize;
    type Item;

    /// Allocate
    unsafe fn alloc(&self, new: PointRange);
    /// Mark as dead/ready to be freed.
    /// Return `Some(point)`, if the slot is in control and should deallocate that `range`.
    /// `Range::EMPTY` is used when either only slot, or data is not allocated
    unsafe fn mark_dead(&self, dead: PointRange) -> Option<PointRange>;
    /// Free the range and return the point until which it should actually be freed.
    unsafe fn dealloc(&self, free: PointRange) -> Point;
}

struct Handle<'a, T: Message> {
    buf: &'a Buffer<T>,
    range: PointRange,
}

#[derive(Debug, PartialEq, PartialOrd, Ord, Eq)]
pub struct Point {
    pub slot: usize,
    pub data: usize,
}

#[derive(Debug, PartialEq, PartialOrd, Ord, Eq)]
pub struct PointRange {
    pub slot: Range,
    pub data: Range,
}

impl PointRange {
    unsafe fn to(&self) -> Point {
        let PointRange {
            slot: Range { from: _, to: slot },
            data: Range { from: _, to: data },
        } = self;

        Point {
            slot: *slot,
            data: *data,
        }
    }
}

/// Range of values. Might wrap.
#[derive(Debug, PartialEq, PartialOrd, Ord, Eq)]
pub struct Range {
    /// inclusive start of the range
    pub from: usize,
    /// exclusive end of the range
    pub to: usize,
}

impl Range {
    pub const EMPTY: Self = Range { from: 0, to: 0 };
}

impl<T: Message> Buffer<T> {
    pub fn new(message: T, len: usize) -> Self {
        let buf = unsafe { std::alloc::alloc(Layout::array::<T::Item>(len).unwrap()) };

        Self {
            buf: std::ptr::slice_from_raw_parts_mut(buf as *mut T::Item, len),
            data: message,
            slot_next: AtomicUsize::new(0),
            slot_free: AtomicUsize::new(0),
            data_next: AtomicUsize::new(0),
            data_free: AtomicUsize::new(0),
        }
    }

    fn allocated_range(&self) -> PointRange {
        PointRange {
            slot: Range {
                from: self.slot_free.load(Ordering::Relaxed),
                to: self.slot_next.load(Ordering::Relaxed),
            },
            data: Range {
                from: self.data_free.load(Ordering::Relaxed),
                to: self.data_next.load(Ordering::Relaxed),
            },
        }
    }

    /// # Safety
    ///
    /// Caller has to guarantie that:
    /// - slot_next..slot_new is valid
    /// - data_next..data_new is valid
    ///
    /// Those ranges are valid if: TODO actually define that
    pub unsafe fn allocate(&self, range: PointRange) -> Option<Handle<'_, T>> {
        match self.slot_next.compare_exchange(
            range.slot.from,
            range.slot.to,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            // Slots were successfully allocated!
            // This function is now responsible for making sure it doesn't accidentally loose track
            // of them!
            //
            // Invariant: `slot_next..slot_new` is marked as `.is_alive()`
            Ok(_) => {}

            // Some other thread allocated an id between us fetching `slot_next` and replacing it
            // with `slot_new`
            Err(_) => return None,
        }

        match self.data_next.compare_exchange(
            range.data.from,
            range.data.to,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            // return handle
            Ok(_) => Some(Handle { buf: self, range }),
            // mark slots as dead and return none
            Err(_) => {
                unsafe {
                    self.mark_dead(PointRange {
                        slot: range.slot,
                        data: Range::EMPTY,
                    });
                }

                None
            }
        }
    }

    /// # Safety
    ///
    /// Caller has to guarantie that they have control over the range.
    pub unsafe fn mark_dead(&self, range: PointRange) {
        unsafe {
            if let Some(free) = self.data.mark_dead(range) {
                self.dealloc(free);
            }
        };
    }

    /// # Safety
    ///
    /// Caller has to guarantie that:
    /// - has control over `slot_free` and `slot_free` is alive
    /// - has control over the slots they are deallocating
    /// - has control over the data they are deallocating
    pub unsafe fn dealloc(&self, free: PointRange) {
        let free = unsafe { self.data.dealloc(free) };
        self.slot_free.store(free.slot, Ordering::Release);
        self.data_free.store(free.data, Ordering::Release);
    }
}

impl<T: Message> Drop for Buffer<T> {
    fn drop(&mut self) {
        let ptr = self.buf as *mut u8;
        let len = self.buf.len();

        unsafe {
            std::alloc::dealloc(ptr, Layout::array::<T::Item>(len).unwrap());
        }
    }
}
