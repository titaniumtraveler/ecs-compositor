use crate::{
    RawSliceExt,
    primitives::{Value, Result},
    wl_display,
};
use std::os::unix::prelude::RawFd;

/// The value is the 32-bit value of the signed int.
#[derive(Debug)]
pub struct Int(pub i32);

/// The value is the 32-bit value of the unsigned int.
#[derive(Debug)]
pub struct UInt(pub u32);

impl<'data> Value<'data> for Int {
    fn len(&self) -> u32 {
        4
    }

    unsafe fn read(data: &mut *const [u8], _: &mut *const [RawFd]) -> Result<Self> {
        let i32 = unsafe {
            data.split_at(4)
                .ok_or(wl_display::Error::InvalidMethod.msg("failed to read int"))?
                .cast::<i32>()
                .read()
        };

        Ok(Self(i32))
    }

    unsafe fn write<'a>(&self, data: &mut *mut [u8], _: &mut *mut [RawFd]) -> Result<()> {
        unsafe {
            data.split_at(4)
                .ok_or(wl_display::Error::Implementation.msg("not enough write buffer for int"))?
                .cast::<i32>()
                .write(self.0);
        }
        Ok(())
    }
}

impl<'data> Value<'data> for UInt {
    fn len(&self) -> u32 {
        4
    }

    unsafe fn read(data: &mut *const [u8], _: &mut *const [RawFd]) -> Result<Self> {
        let u32 = unsafe {
            data.split_at(4)
                .ok_or(wl_display::Error::InvalidMethod.msg("failed to read int"))?
                .cast::<u32>()
                .read()
        };

        Ok(Self(u32))
    }

    unsafe fn write<'a>(&self, data: &mut *mut [u8], _: &mut *mut [RawFd]) -> Result<()> {
        unsafe {
            data.split_at(4)
                .ok_or(wl_display::Error::Implementation.msg("not enough write buffer for int"))?
                .cast::<u32>()
                .write(self.0);
        }
        Ok(())
    }
}
