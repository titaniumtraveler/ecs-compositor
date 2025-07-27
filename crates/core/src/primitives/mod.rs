use crate::wl_display::{self, WlDisplay};
use std::{
    ops::Add,
    os::fd::RawFd,
    ptr::{self, NonNull, slice_from_raw_parts_mut},
};

mod array;
mod enum_;
mod fd;
mod fixed;
mod int;
mod object;

pub use self::{
    array::{Array, String},
    enum_::Enum,
    fd::Fd,
    fixed::Fixed,
    int::{Int, UInt},
    object::{NewId, NewIdDyn, Object},
};

#[allow(clippy::len_without_is_empty)] // We are not a collection
pub trait Primitive<'data>: Sized {
    fn len(&self) -> u32;

    /// Panicks if `fds` is empty when calling [`Fd::read()`].
    /// (The number of fds that are needed are known when parsing the message, so no reason not to
    /// keep this static.)
    fn read(data: &mut &'data [u8], fds: &mut &[RawFd]) -> Result<Self>;
    /// Write primitive to buffer. Panicks if `data` shorter than [`Self::len()`], or if `fds` is
    /// empty when calling [`Fd::write()`].
    fn write(&self, data: &mut ThickPtr<u8>, fds: &mut ThickPtr<RawFd>) -> Result<()>;
}

pub type Result<T> = std::result::Result<T, Error>;
pub struct Error {
    pub err: wl_display::Error,
    pub msg: &'static str,
}

impl From<Error> for crate::Error {
    fn from(value: Error) -> Self {
        WlDisplay::OBJECT.err(value.err as u32, value.msg)
    }
}

/// # SAFETY
///
/// This type is inherently unsafe and should be used with care!
pub struct ThickPtr<T> {
    pub ptr: NonNull<T>,
    pub len: usize,
}

impl<T> ThickPtr<T> {
    /// # Safety
    ///
    /// 1. `self` has to point a valid empty allocation that is writable and has to be aligned to `T`.
    /// 2. `src.len < self.len()` has to be true.
    /// 3. `self` and `src` have to be non-overlapping. (This is implied by `1.`)
    #[inline]
    pub unsafe fn write_slice<'a>(&mut self, src: &[T]) -> &'a mut [T]
    where
        T: Copy,
    {
        debug_assert!(src.len() < self.len);

        let dst = self.ptr;
        let (src, len) = (src.as_ptr(), src.len());

        // SAFETY:
        // Behavior is undefined if any of the following conditions are violated:
        //
        // - `src` is a slice, so it is valid to read form for `len` items.
        // - The caller guarantees that dst is valid for writing for `len` items.
        // - The caller guaranties that src and dst are properly aligned.
        // - The caller guaranties that src and dst are non-overlapping.
        unsafe {
            ptr::copy_nonoverlapping(src, dst.as_ptr(), len);
        }

        // SAFETY: Caller guaranties safety.
        unsafe {
            self.advance(len);
        }

        // SAFETY: Constructing this `slice` from `dst` and `len` is safe, because it points to
        // valid and initialized data that we have mutable access to.
        unsafe { &mut *slice_from_raw_parts_mut(dst.as_ptr(), len) }
    }

    /// # Safety
    /// See [`Self::write_slice()`]
    pub unsafe fn write(&mut self, val: T) {
        unsafe {
            self.ptr.write(val);
            self.advance(1);
        }
    }

    /// # SAFETY
    /// See [`*mut T::add()`]
    pub unsafe fn advance(&mut self, count: usize) {
        unsafe {
            self.ptr = self.ptr.add(count);
            self.len = self.len.add(count);
        }
    }
}

impl ThickPtr<u8> {
    /// # Safety
    /// See [`Self::write_slice()`]
    #[inline]
    pub unsafe fn write_4_bytes(&mut self, bytes: [u8; 4]) {
        let bytes = &bytes;
        let dst = self.ptr;

        unsafe {
            ptr::copy_nonoverlapping(bytes.as_ptr(), dst.as_ptr(), 4);
        }

        unsafe {
            self.advance(4);
        }
    }

    /// # Safety
    /// See [`Self::write_slice()`]
    pub unsafe fn write_zeros(&mut self, count: usize) {
        unsafe {
            ptr::write_bytes(self.ptr.as_ptr(), 0u8, count);
            self.advance(count);
        }
    }
}

#[inline]
pub(crate) fn read_4_bytes(data: &mut &[u8]) -> Option<[u8; 4]> {
    match data[..] {
        [a, b, c, d, ref tail @ ..] => {
            *data = tail;
            Some([a, b, c, d])
        }
        _ => None,
    }
}
