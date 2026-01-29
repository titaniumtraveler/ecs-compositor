use crate::wl_display;
use std::{io, os::fd::RawFd};

pub mod fmt;

// Module to prevent name collisions with the contained types.
mod inner {
    #![allow(non_camel_case_types)]

    pub(super) mod array;
    pub(super) mod enumeration;
    pub(super) mod fd;
    pub(super) mod fixed;
    pub(super) mod int;
    pub(super) mod object;
}

pub use self::inner::{
    array::{array, string},
    enumeration::enumeration,
    fd::fd,
    fixed::fixed,
    int::{int, uint},
    object::{new_id, new_id_dyn, object},
};

#[allow(clippy::len_without_is_empty)] // We are not a collection
pub trait Value<'data>: Sized {
    /// Number of FD args of this value.
    const FDS: usize;
    fn len(&self) -> u32;

    /// # Safety
    ///
    /// - `data` and `fds` have to point to a valid buffer to read from.
    /// - `data` has to be aligned to a 4 byte boundary.
    /// - All FDs in `fds` have to be valid.
    ///
    /// - Implementors should not assume that the buffer has enough space for `Self`!
    ///   In case there is not enough space, an error should be thrown!
    ///
    ///   Note that parts of the value might still have been read from the buffer, which means the
    ///   `data` and/or `fds` pointers were advanced forward!
    ///   To prevent data confusions caused by that, either roll `data` and `fds` back on errors,
    ///   or try to infer the message length from things like the length declared in the wayland
    ///   header, or if possible the static length of the message!
    unsafe fn read(data: &mut *const [u8], fds: &mut *const [RawFd]) -> Result<Self>;

    /// # Safety
    ///
    /// - `data` and `fds` have to point to a valid buffer to write to.
    /// - `data` has to be aligned to a 4 byte boundary.
    /// - All FDs in `fds` have to be valid.
    ///
    /// - Implementors should not assume that the buffer has enough space for `Self`!
    ///   In case there is not enough space, an error should be thrown!
    ///
    ///   Note that parts of the value might still have been written to the buffer and therefore advanced it forward!
    ///   Make sure to check [`Self::len()`], or if necessary rollback the `data` and `fds`
    ///   pointers to before the attempted write to prevent partial writes to be actually sent!
    unsafe fn write(&self, data: &mut *mut [u8], fds: &mut *mut [RawFd]) -> Result<()>;
}

pub type Result<T> = std::result::Result<T, Error>;
pub struct Error {
    pub err: wl_display::enumeration::error,
    pub msg: &'static str,
}

impl From<Error> for io::Error {
    fn from(Error { err, msg }: Error) -> Self {
        io::Error::other(format!("{err}: {msg}"))
    }
}

impl From<Error> for crate::wl_display::event::error {
    fn from(value: Error) -> Self {
        wl_display::OBJECT.err(uint(value.err.to_u32()), value.msg)
    }
}

pub const fn align<const ALIGN: u32>(len: u32) -> u32 {
    (len + ALIGN - 1) & !(ALIGN - 1)
}
