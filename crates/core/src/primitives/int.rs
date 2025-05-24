use crate::{
    primitives::{Primitive, Result, ThickPtr, read_4_bytes},
    wl_display,
};
use std::os::unix::prelude::RawFd;

/// The value is the 32-bit value of the signed int.
pub struct Int(pub i32);

/// The value is the 32-bit value of the unsigned int.
pub struct UInt(pub u32);

impl<'data> Primitive<'data> for Int {
    fn len(&self) -> u32 {
        4
    }

    fn read(data: &mut &'data [u8], _: &mut &[RawFd]) -> Result<Self> {
        let bytes =
            read_4_bytes(data).ok_or(wl_display::Error::InvalidMethod.msg("failed to read int"))?;

        Ok(Self(i32::from_ne_bytes(bytes)))
    }

    fn write<'a>(&self, data: &mut ThickPtr<u8>, _: &mut ThickPtr<RawFd>) -> Result<()> {
        unsafe {
            data.write_4_bytes(self.0.to_ne_bytes());
        }
        Ok(())
    }
}

impl<'data> Primitive<'data> for UInt {
    fn len(&self) -> u32 {
        4
    }

    fn read(data: &mut &'data [u8], _: &mut &[RawFd]) -> Result<Self> {
        let bytes = read_4_bytes(data)
            .ok_or(wl_display::Error::InvalidMethod.msg("failed to read uint"))?;

        Ok(Self(u32::from_ne_bytes(bytes)))
    }

    fn write<'a>(&self, data: &mut ThickPtr<u8>, _: &mut ThickPtr<RawFd>) -> Result<()> {
        unsafe {
            data.write_4_bytes(self.0.to_ne_bytes());
        }
        Ok(())
    }
}
