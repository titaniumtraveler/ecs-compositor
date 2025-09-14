//! Stripped down impl of [`WlDisplay`] for error reporting

use crate::{Interface, interface::Opcode, object};
use std::num::NonZero;

#[allow(non_camel_case_types)]
pub enum wl_display {}

/// `wl_display` is **always** available at id 1
pub const OBJECT: object = object::from_id(NonZero::new(1).unwrap());

impl Interface for wl_display {
    const NAME: &str = "wl_display";
    const VERSION: u32 = 1;

    type Error = self::enumeration::error;

    type Request = Request;
    type Event = Event;
}

pub enum Request {}
impl Opcode for Request {
    fn from_u16(i: u16) -> Result<Self, u16> {
        Err(i)
    }

    fn to_u16(self) -> u16 {
        unreachable!()
    }
}

#[repr(u16)]
#[allow(non_camel_case_types)]
pub enum Event {
    error = 0,
}

impl Opcode for Event {
    fn from_u16(i: u16) -> Result<Self, u16> {
        match i {
            0 => Ok(Self::error),
            err => Err(err),
        }
    }

    fn to_u16(self) -> u16 {
        self as _
    }
}

pub mod enumeration {
    use crate::{Value, enumeration, primitives, uint};
    use std::os::fd::RawFd;

    /// global error values
    ///
    /// These errors are global and can be emitted in response to any
    /// server request.
    #[derive(Debug, Clone, Copy)]
    #[repr(u32)]
    #[allow(non_camel_case_types)]
    pub enum error {
        ///server couldn't find object
        invalid_object = 0,
        ///method doesn't exist on the specified interface or malformed request
        invalid_method = 1,
        ///server is out of memory
        no_memory = 2,
        ///implementation error in compositor
        implementation = 3,
    }

    impl error {
        pub fn msg(self, msg: &'static str) -> primitives::Error {
            primitives::Error { err: self, msg }
        }
    }

    impl enumeration for error {
        fn from_u32(int: u32) -> Option<Self> {
            match int {
                0 => Some(Self::invalid_object),
                1 => Some(Self::invalid_method),
                2 => Some(Self::no_memory),
                3 => Some(Self::implementation),
                _ => None,
            }
        }

        fn to_u32(&self) -> u32 {
            *self as u32
        }

        fn since_version(&self) -> u32 {
            1
        }
    }

    impl<'data> Value<'data> for error {
        fn len(&self) -> u32 {
            uint(self.to_u32()).len()
        }

        unsafe fn read(
            data: &mut *const [u8],
            fds: &mut *const [RawFd],
        ) -> primitives::Result<Self> {
            unsafe {
                Self::from_u32(uint::read(data, fds)?.0)
                    .ok_or(error::implementation.msg("invalid u32 value for `wl_display::error`"))
            }
        }

        unsafe fn write(
            &self,
            data: &mut *mut [u8],
            fds: &mut *mut [RawFd],
        ) -> primitives::Result<()> {
            unsafe { uint(self.to_u32()).write(data, fds) }
        }
    }
}
