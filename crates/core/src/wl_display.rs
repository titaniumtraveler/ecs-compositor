//! Stripped down impl of [`WlDisplay`] for error reporting

use crate::{Interface, Value, enumeration, object, primitives, uint, wl_display};
use std::{num::NonZero, os::unix::prelude::RawFd};

pub enum WlDisplay {}

impl WlDisplay {
    /// `wl_display` is **always** available at id 1
    pub const OBJECT: object = object::from_id(NonZero::new(1).unwrap());
}

impl Interface for WlDisplay {
    const NAME: &str = "wl_display";
    const VERSION: u32 = 1;

    type Error = wl_display::Error;
}

/// global error values
///
/// These errors are global and can be emitted in response to any
/// server request.
#[derive(Debug, Clone, Copy)]
#[repr(u32)]
pub enum Error {
    ///server couldn't find object
    InvalidObject = 0,
    ///method doesn't exist on the specified interface or malformed request
    InvalidMethod = 1,
    ///server is out of memory
    NoMemory = 2,
    ///implementation error in compositor
    Implementation = 3,
}

impl Error {
    pub fn msg(self, msg: &'static str) -> primitives::Error {
        primitives::Error { err: self, msg }
    }
}

impl enumeration for Error {
    fn from_u32(int: u32) -> Option<Self> {
        match int {
            0 => Some(Self::InvalidObject),
            1 => Some(Self::InvalidMethod),
            2 => Some(Self::NoMemory),
            3 => Some(Self::Implementation),
            _ => None,
        }
    }

    fn to_u32(&self) -> u32 {
        *self as u32
    }
}

impl<'data> Value<'data> for Error {
    fn len(&self) -> u32 {
        uint(self.to_u32()).len()
    }

    unsafe fn read(data: &mut *const [u8], fds: &mut *const [RawFd]) -> primitives::Result<Self> {
        unsafe {
            Self::from_u32(uint::read(data, fds)?.0)
                .ok_or(Error::Implementation.msg("invalid u32 value for `wl_display::error`"))
        }
    }

    unsafe fn write(&self, data: &mut *mut [u8], fds: &mut *mut [RawFd]) -> primitives::Result<()> {
        unsafe { uint(self.to_u32()).write(data, fds) }
    }
}
