use crate::{
    RawSliceExt,
    primitives::{Result, Value},
    wl_display::enumeration::error,
};
use std::os::unix::prelude::RawFd;

/// The value is the 32-bit value of the signed int.
#[derive(Debug, Clone, Copy)]
pub struct int(pub i32);

/// The value is the 32-bit value of the unsigned int.
#[derive(Debug, Clone, Copy)]
pub struct uint(pub u32);

impl<'data> Value<'data> for int {
    const FDS: usize = 0;
    fn len(&self) -> u32 {
        4
    }

    unsafe fn read(data: &mut *const [u8], _: &mut *const [RawFd]) -> Result<Self> {
        let i32 = unsafe {
            data.split_at(4)
                .ok_or(error::invalid_method.msg("failed to read int"))?
                .cast::<i32>()
                .read()
        };

        Ok(Self(i32))
    }

    unsafe fn write<'a>(&self, data: &mut *mut [u8], _: &mut *mut [RawFd]) -> Result<()> {
        unsafe {
            data.split_at(4)
                .ok_or(error::implementation.msg("not enough write buffer for int"))?
                .cast::<i32>()
                .write(self.0);
        }
        Ok(())
    }
}

impl<'data> Value<'data> for uint {
    const FDS: usize = 0;
    fn len(&self) -> u32 {
        4
    }

    unsafe fn read(data: &mut *const [u8], _: &mut *const [RawFd]) -> Result<Self> {
        let u32 = unsafe {
            data.split_at(4)
                .ok_or(error::invalid_method.msg("failed to read int"))?
                .cast::<u32>()
                .read()
        };

        Ok(Self(u32))
    }

    unsafe fn write<'a>(&self, data: &mut *mut [u8], _: &mut *mut [RawFd]) -> Result<()> {
        unsafe {
            data.split_at(4)
                .ok_or(error::implementation.msg("not enough write buffer for int"))?
                .cast::<u32>()
                .write(self.0);
        }
        Ok(())
    }
}
