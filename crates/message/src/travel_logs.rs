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

trait Message {
    const MAX_SLOTS: usize;
    type Item;
}

struct Handle<'a, T: Message> {
    buf: &'a Buffer<T>,

    slot_start: usize,
    slot_end: usize,

    data_start: usize,
    data_end: usize,
}

impl<T: Message> Buffer<T> {
    fn new(message: T, len: usize) -> Self {
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

    /// # Safety
    ///
    /// Caller has to guarantie that:
    /// - slot_next..slot_new is valid
    /// - data_next..data_new is valid
    ///
    /// Those ranges are valid if: TODO actually define that
    unsafe fn allocate(
        &self,
        slot_next: usize,
        slot_new: usize,
        data_next: usize,
        data_new: usize,
    ) -> Option<Handle<'_, T>> {
        match self.slot_next.compare_exchange(
            slot_next,
            slot_new,
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
            data_next,
            data_new,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            // return handle
            Ok(_) => Some(Handle {
                buf: self,
                slot_start: slot_next,
                slot_end: slot_new,
                data_start: data_next,
                data_end: data_new,
            }),
            // play dead and return none
            Err(_) => {
                // TODO: mark slots as dead
                eprintln!("marking slots as dead is not yet implemented!!!");
                None
            }
        }
    }

    /// # Safety
    ///
    /// Caller has to guarantie that:
    /// - has control over `slot_free` and `slot_free` is alive
    /// - has control over the slots they are deallocating
    /// - has control over the data they are deallocating
    unsafe fn dealloc(&self, slot_free: usize, data_free: usize) {
        // TODO: actually mark those slots as alive again
        // TODO: also search for the next active slot instead of using just the end
        self.slot_free.store(slot_free, Ordering::Release);
        self.data_free.store(data_free, Ordering::Release);
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
