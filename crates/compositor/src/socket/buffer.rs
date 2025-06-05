use std::{
    cmp,
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

    /// Index of the first active message, until which new messages can be written
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

    fn deallocate(&self, index: usize) {
        if self.write_until.load(Ordering::Acquire) != index {
            // Mark message as tombstone and exit
            unsafe { &*self.buf.add(index) }
                .is_active
                .store(false, Ordering::Release);

            return;
        }

        let mut write_next = loop {
            match self.write_next.load(Ordering::Acquire) {
                // Spin until we get the actual value of `self.write_next`
                PROCESSING => std::hint::spin_loop(),
                write_next => break write_next,
            }
        };

        let mut cleanup_until = index + 1;
        'cleanup: loop {
            if cleanup_until == self.capacity {
                // wrap around
                cleanup_until = 0;
            }

            if cleanup_until == write_next || cleanup_until == index {
                // We have arrived either at the last message, or wrapped around to ourselves,
                // If we wrapped around that means the queue is full.

                // If the queue is full, we *theoretically* don't need to take
                loop {
                    match self.write_next.compare_exchange_weak(
                        write_next,
                        PROCESSING,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    ) {
                        Ok(_) => break,
                        Err(PROCESSING) => std::hint::spin_loop(),
                        Err(actual) => {
                            if cleanup_until == actual {
                                // spurious error to store the `PROCESSING`, so we retry
                                std::hint::spin_loop();
                                continue;
                            }

                            // a message has been added between us getting `write_next` and now, so
                            // we continue doing cleanup
                            write_next = actual;
                            continue 'cleanup;
                        }
                    }
                }

                // Since we hold a lock preventing more messages to be added to the queue and
                // we are currently deallocating the last message, we can just reset the whole
                // queue.

                self.data.write_until.store(0, Ordering::Release);
                self.data.write_next.store(0, Ordering::Release);

                self.fds.write_until.store(0, Ordering::Release);
                self.fds.write_next.store(0, Ordering::Release);

                self.write_until.store(0, Ordering::Release);

                // Release the lock
                self.write_next.store(0, Ordering::Release);

                break;
            }

            let message = unsafe { self.buf.add(cleanup_until) };
            let is_active = unsafe { &(*message).is_active };
            if is_active.load(Ordering::Acquire) {
                // We arrived at an active message

                // SAFETY:
                // The message is active, and we have control over the queue, so getting a
                // immutable reference to it is fine.
                let Message {
                    data_start,
                    fds_start,
                    ..
                } = unsafe { &*message };

                // Mark `self.data` and `self.fds` as free until the contents of the message at
                // `cleanup_until`.
                //
                // This is safe because all messages in-between are marked as not active,
                // so there are no references to it anymore and the space is safe to be re-used.
                self.data.write_until.store(*data_start, Ordering::Release);
                self.fds.write_until.store(*fds_start, Ordering::Release);

                // Give over control to the message
                self.write_until.store(cleanup_until, Ordering::Release);

                // Make sure the message we handed control is still active,
                // since us checking `message.is_active` above.
                if is_active.load(Ordering::Acquire) {
                    break;
                } else {
                    if self.write_until.load(Ordering::Acquire) != cleanup_until {
                        // `write_until` moved, which means someone else will do the reallocation
                    }
                    // FIX: There is a race condition here, where message queue has already wrapped
                    // around at this point, which would mean that there are two threads trying to
                    // do the cleanup in parallel, which is undefined behavior.
                    // For now we just assume that never actually happens.
                }
            }

            // Mark message as active again in preparations of the instant activation when
            // reallocating this space.
            //
            // This is important, because otherwise an just allocated message could get marked as
            // deallocated by this loop, which would mean it is still perceived as allocated by the
            // message handle, while being perceived as space that new messages can be allocated into
            // by the rest of the queue, which is a trivial use-after-free.
            is_active.store(true, Ordering::Release);
            cleanup_until += 1;
        }
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

    data_start: usize,
    fds_start: usize,
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
        let mut new_write_next;

        loop {
            'enough_space: {
                if write_until <= write_next {
                    let available_space = self.capacity - write_next;
                    if len < available_space {
                        new_write_next = write_next + len;
                        break 'enough_space;
                    }

                    if write_next == self.capacity {
                        // The queue is marked as full, so the allocation failed
                        return None;
                    }

                    // Wrap around and try again
                    write_next = 0;
                }

                let available_space = write_until - write_next;
                match available_space.cmp(&len) {
                    cmp::Ordering::Less => return None,
                    // Marking the queue as full.
                    cmp::Ordering::Equal => new_write_next = self.capacity,
                    cmp::Ordering::Greater => new_write_next = write_next + len,
                }
            }

            // Actually allocate our new data
            match self.write_next.compare_exchange_weak(
                write_next,
                new_write_next,
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

        // SAFETY:
        // We have just allocated the buffer, so handing out a mutable reference to it, which will
        // be exclusively be used by holders of the handle, is fine.
        let data = unsafe { ptr::slice_from_raw_parts_mut(self.buf.add(write_next), len) };

        Some(SubqueueHandle {
            queue: self,
            index: write_next,
            data,
        })
    }
}
