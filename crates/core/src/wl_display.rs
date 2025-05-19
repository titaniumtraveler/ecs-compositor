//! Stripped down impl of [`WlDisplay`]

use crate::{
    Interface,
    primitives::{Enum, Object},
};
use std::num::NonZero;

pub enum WlDisplay {}

impl WlDisplay {
    /// `wl_display` is **always** available at id 1
    pub const OBJECT: Object<WlDisplay> = Object::from_id(NonZero::new(1).unwrap());
}

impl Interface for WlDisplay {
    const NAME: &str = "wl_display";
    const VERSION: u32 = 1;

    type Error = Error;
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
    pub fn msg(self, msg: &'static str) -> crate::Error<WlDisplay> {
        WlDisplay::OBJECT.err(self, msg)
    }
}

impl Enum for Error {
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
