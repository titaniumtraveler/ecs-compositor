use std::{
    os::fd::RawFd,
    ptr,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

struct MessageQueue {
    buf: *mut Message,
    capacity: usize,

    /// Index of where to write the next message
    ///
    /// Is guarantied to be either
    /// - `write_next < capacity`
    /// - `write_next == self.capacity` to mark the queue as full
    /// - `write_next == PROCESSING` to mark that another writer is currently allocating a message
    write_next: AtomicUsize,

    /// Index of the first active message, util which new messages can be written
    write_until: AtomicUsize,

    data: Subqueue<u8>,
    fds: Subqueue<RawFd>,
}

const PROCESSING: usize = usize::MAX;

impl MessageQueue {
    fn allocate_message(&self, data: usize, fds: usize) -> Option<MessageHandle<'_>> {
        let mut write_next = self.write_next.load(Ordering::Acquire);

        loop {
            // Spin until we have an unlocked `self.write` index
            match write_next {
                PROCESSING => {
                    std::hint::spin_loop();
                    write_next = self.write_next.load(Ordering::Acquire);
                    continue;
                }
                // The queue is full, so we bail
                next if next == self.capacity => return None,
                next => write_next = next,
            }

            // Try locking `self.write`
            match self.write_next.compare_exchange_weak(
                write_next,
                PROCESSING,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(actual) => write_next = actual,
            }
        }

        let index = write_next;
        let data_handle = self.data.allocate(data)?;
        let fds_handle = self.fds.allocate(fds)?;

        // SAFETY: We have immutable access to `self`, and have just allocated the new message,
        // so handing out an immutable reference to it is fine.
        let message = unsafe { &*self.buf.add(write_next) };

        write_next = if write_next + 1 < self.capacity {
            write_next + 1
        } else {
            0
        };

        // Use `Ordering::Acquire` to prevent a race condition between the cleanup of old messages
        // and setting `write_next` here, which would lead to a deadlock
        if write_next == self.write_until.load(Ordering::Acquire) {
            // Mark the queue as full
            write_next = self.capacity;
        }

        self.write_next.store(write_next, Ordering::Release);

        Some(MessageHandle {
            queue: self,
            index,
            message,
            data: data_handle.data,
            fds: fds_handle.data,
        })
    }
}

struct MessageHandle<'a> {
    queue: &'a MessageQueue,
    index: usize,

    message: &'a Message,
    data: *mut [u8],
    fds: *mut [RawFd],
}

struct Message {
    is_active: AtomicBool,

    data_index: usize,
    fd_index: usize,
}

struct Subqueue<T> {
    buf: *mut T,
    capacity: usize,

    write_next: AtomicUsize,
    write_until: AtomicUsize,
}

struct SubqueueHandle<'a, T> {
    queue: &'a Subqueue<T>,
    index: usize,

    data: *mut [T],
}

impl<T> Subqueue<T> {
    fn allocate(&self, len: usize) -> Option<SubqueueHandle<'_, T>> {
        let mut write_next = self.write_next.load(Ordering::Acquire);
        let mut write_until = self.write_until.load(Ordering::Acquire);

        loop {
            'enough_space: {
                if write_until <= write_next {
                    let available_space = self.capacity - write_next;
                    if len < available_space {
                        break 'enough_space;
                    }

                    if write_until != 0 {
                        // Wrap around
                        write_next = 0;
                    } else {
                        // The queue is full, so the wrap around failed.
                        return None;
                    }

                    // Try again with the wrap around
                }

                let available_space = write_until - write_next;
                if available_space < len {
                    return None;
                }
            }

            // Actually allocate our new data
            match self.write_next.compare_exchange_weak(
                write_next,
                write_next + len,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                // In case the replacing actually failed, we need to re-fetch `write_until` and try again
                Err(actual) => {
                    write_next = actual;
                    write_until = self.write_until.load(Ordering::Acquire);
                }
            }
        }

        let index = write_next;
        // SAFETY:
        // We have just allocated the buffer, so handing out a mutable reference to it, which will
        // be exclusively be used by holders of the handle, is fine.
        let data = unsafe { ptr::slice_from_raw_parts_mut(self.buf.add(index), len) };

        Some(SubqueueHandle {
            queue: self,
            index,
            data,
        })
    }
}
