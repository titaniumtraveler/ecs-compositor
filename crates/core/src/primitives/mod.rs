use crate::{Result, wl_display::WlDisplay};
use std::{
    mem::{self, MaybeUninit},
    os::fd::RawFd,
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
    object::{NewId, Object},
};

#[allow(clippy::len_without_is_empty)] // We are not a collection
pub trait Primitive<'data>: Sized {
    fn len(&self) -> u32;

    /// Panicks if `fds` is empty when calling [`Fd::read()`].
    /// (The number of fds that are needed are known when parsing the message, so no reason not to
    /// keep this static.)
    fn read(data: &mut &'data [u8], fds: &mut &[RawFd]) -> Result<Self, WlDisplay>;
    /// Write primitive to buffer. Panicks if `data` shorter than [`Self::len()`], or if `fds` is
    /// empty when calling [`Fd::write()`].
    fn write<'o: 'i, 'i>(
        &self,
        data: &'o mut &'i mut [MaybeUninit<u8>],
        fds: &'o mut &'i mut [MaybeUninit<RawFd>],
    ) -> Result<(), WlDisplay>;
}

/// See [`slice::write_copy_of_slice()`]
///
/// # Panics
///
/// If `slice.len() != src.len()`
// TODO: Wait for stabilization of https://github.com/rust-lang/rust/issues/79995
#[inline]
pub(crate) fn write_copy_of_slice<'a, T>(slice: &'a mut [MaybeUninit<T>], src: &[T]) -> &'a mut [T]
where
    T: Copy,
{
    // SAFETY: &mut [T] and &mut [MaybeUninit<T>] have the same layout
    let uninit_src: &[MaybeUninit<T>] = unsafe { mem::transmute(src) };

    slice.copy_from_slice(uninit_src);

    // SAFETY: casting `slice` to a `*mut [T]` is safe since valid elements have just been copied
    // into the slice, so it is initialized, and `MaybeUninit` is guaranteed to have the same layout as `T`.
    // The pointer obtained is valid since it refers to memory owned by `slice` which is a
    // mutable reference and thus guaranteed to be valid for writes.
    unsafe { &mut *(slice as *mut [MaybeUninit<T>] as *mut [T]) }
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

#[inline]
pub(crate) fn write_4_bytes<'o: 'i, 'i>(data: &'o mut &'i mut [MaybeUninit<u8>], bytes: [u8; 4]) {
    let (buf, tail) = data.split_at_mut(4);
    *data = tail;

    write_copy_of_slice(buf, &bytes);
}
