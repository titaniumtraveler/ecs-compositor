use std::{
    alloc::Layout,
    fmt::Debug,
    ops::Bound,
    sync::atomic::{AtomicUsize, Ordering},
};

#[cfg(test)]
mod tests;

pub struct Buffer<T: Metadata> {
    buf: *mut [T::Data],
    metadata: T,

    slot_next: AtomicUsize,
    slot_free: AtomicUsize,

    data_next: AtomicUsize,
    data_free: AtomicUsize,
}

unsafe impl<T: Metadata + Sync> Sync for Buffer<T> {}
unsafe impl<T: Metadata + Send> Send for Buffer<T> {}

impl<T: Metadata + Debug> Debug for Buffer<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Buffer")
            .field("buf", &self.buf)
            .field("metadata", &self.metadata)
            .field("slot_next", &self.slot_next)
            .field("slot_free", &self.slot_free)
            .field("data_next", &self.data_next)
            .field("data_free", &self.data_free)
            .finish()
    }
}

/// # Safety
///
/// Implementing this incorrectly will cause undefined behavior
pub unsafe trait Metadata {
    type Handle;
    type Data;

    /// Get `slot` and `data` capacity.
    fn capacity(&self) -> Point;

    /// Allocate
    ///
    /// # Safety
    ///
    /// TODO
    unsafe fn alloc(&self, new: PointRange) -> Self::Handle;
    /// Mark as dead/ready to be freed.
    /// Return `Some(point)`, if the slot is in control and should deallocate that `range`.
    /// `Range::EMPTY` is used when either only slot, or data is not allocated
    ///
    /// # Safety
    ///
    /// Caller has to guarantie that they have control over the range.
    unsafe fn mark_dead(
        &self,
        allocated: PointRange,
        dead: PointRange,
        handle: Self::Handle,
    ) -> Option<PointRange>;
    /// Free the range and return the point until which it should actually be freed.
    ///
    /// # Safety
    ///
    /// Caller has to guarantie that they have control over the range.
    /// And that they have the last allocated object.
    unsafe fn dealloc(&self, free: PointRange) -> Point;
}

#[derive(Debug)]
pub struct Handle<'a, T: Metadata> {
    buf: &'a Buffer<T>,
    range: PointRange,
    handle: T::Handle,
}

impl<'a, T: Metadata> Handle<'a, T> {
    pub fn into_raw(self) -> PointRange {
        let range = self.range;
        std::mem::forget(self);
        range
    }

    pub fn dealloc(self) {
        unsafe { self.buf.mark_dead(self.range, self.handle) };
    }
}

#[derive(Debug, PartialEq, PartialOrd, Ord, Eq)]
pub struct Point {
    pub slot: usize,
    pub data: usize,
}

#[derive(Debug, PartialEq, PartialOrd, Ord, Eq, Clone, Copy)]
pub struct PointRange {
    pub slot: Range,
    pub data: Range,
}

impl PointRange {
    pub fn to(&self) -> Point {
        let PointRange { slot: Range { from: _, upto: slot }, data: Range { from: _, upto: data } } =
            self;

        Point { slot: *slot, data: *data }
    }
}

/// Range of values. Might wrap.
#[derive(Debug, PartialEq, PartialOrd, Ord, Eq, Clone, Copy)]
pub struct Range {
    /// inclusive start of the range
    pub from: usize,
    /// exclusive end of the range
    pub upto: usize,
}

pub type Bounds = (Bound<usize>, Bound<usize>);

impl Range {
    pub const EMPTY: Self = Range { from: 0, upto: 0 };

    pub fn invert(self, capacity: usize) -> Self {
        Self { from: self.upto, upto: self.from.checked_sub(1).unwrap_or(capacity - 1) }
    }

    pub const fn into_ring_bounds(
        self,
        capacity: usize,
    ) -> (std::ops::Range<usize>, Option<std::ops::Range<usize>>) {
        if self.from <= self.upto {
            ((self.from..self.upto), None)
        } else {
            ((self.from..capacity), Some(0..self.upto))
        }
    }
}

impl<T: Metadata> Buffer<T> {
    pub fn new(message: T) -> Self {
        let Point { data: len, .. } = message.capacity();
        let buf = unsafe { std::alloc::alloc(Layout::array::<T::Data>(len).unwrap()) };

        Self {
            buf: std::ptr::slice_from_raw_parts_mut(buf as *mut T::Data, len),
            metadata: message,
            slot_next: AtomicUsize::new(0),
            slot_free: AtomicUsize::new(0),
            data_next: AtomicUsize::new(0),
            data_free: AtomicUsize::new(0),
        }
    }

    /// # Safety
    ///
    /// Must be valid handle
    pub unsafe fn handle_from_raw(&self, range: PointRange, handle: T::Handle) -> Handle<'_, T> {
        Handle { buf: self, range, handle }
    }

    #[allow(dead_code)]
    fn allocated_range(&self) -> PointRange {
        PointRange {
            slot: Range {
                from: self.slot_free.load(Ordering::Relaxed),
                upto: self.slot_next.load(Ordering::Relaxed),
            },
            data: Range {
                from: self.data_free.load(Ordering::Relaxed),
                upto: self.data_next.load(Ordering::Relaxed),
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
            range.slot.upto,
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
            range.data.upto,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            // return handle
            Ok(_) => {
                Some(Handle { buf: self, handle: unsafe { self.metadata.alloc(range) }, range })
            }
            // mark slots as dead and return none
            Err(_) => {
                let handle = unsafe {
                    self.metadata
                        .alloc(PointRange { slot: range.slot, data: Range::EMPTY })
                };
                unsafe {
                    self.mark_dead(PointRange { slot: range.slot, data: Range::EMPTY }, handle);
                }

                None
            }
        }
    }

    /// # Safety
    ///
    /// Caller has to guarantie that they have control over the range.
    pub unsafe fn mark_dead(&self, range: PointRange, handle: T::Handle) {
        unsafe {
            if let Some(free) = self
                .metadata
                .mark_dead(self.allocated_range(), range, handle)
            {
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
        let free = unsafe { self.metadata.dealloc(free) };
        self.slot_free.store(free.slot, Ordering::Release);
        self.data_free.store(free.data, Ordering::Release);
    }
}

impl<T: Metadata> Drop for Buffer<T> {
    fn drop(&mut self) {
        let ptr = self.buf as *mut u8;
        let len = self.buf.len();

        unsafe {
            std::alloc::dealloc(ptr, Layout::array::<T::Data>(len).unwrap());
        }
    }
}
