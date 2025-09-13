use crate::{RawSliceExt, primitives::Result, wl_display};
use std::os::fd::RawFd;

pub trait Enum: Sized {
    fn from_u32(int: u32) -> Option<Self>;
    fn to_u32(&self) -> u32;

    unsafe fn read(data: &mut *const [u8], _: &mut *const [RawFd]) -> Result<Self> {
        Self::from_u32(unsafe {
            data.split_at(4)
                .ok_or(wl_display::Error::InvalidMethod.msg("failed to read enum"))?
                .cast::<u32>()
                .read()
        })
        .ok_or(wl_display::Error::InvalidMethod.msg("invalid enum"))
    }
    unsafe fn write(&self, data: &mut *mut [u8], _: &mut *mut [RawFd]) -> Result<()> {
        unsafe {
            data.split_at(4)
                .ok_or(wl_display::Error::Implementation.msg("not enough buffer space for enum"))?
                .cast::<u32>()
                .write(self.to_u32());
        }
        Ok(())
    }
}

impl Enum for u32 {
    fn from_u32(int: u32) -> Option<Self> {
        Some(int)
    }

    fn to_u32(&self) -> u32 {
        *self
    }
}
