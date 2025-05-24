use crate::{
    primitives::{Result, ThickPtr, read_4_bytes},
    wl_display,
};
use std::os::fd::RawFd;

pub trait Enum: Sized {
    fn from_u32(int: u32) -> Option<Self>;
    fn to_u32(&self) -> u32;

    fn read(data: &mut &[u8], _: &mut &[RawFd]) -> Result<Self> {
        let bytes = read_4_bytes(data)
            .ok_or(wl_display::Error::InvalidMethod.msg("failed to read enum"))?;

        Self::from_u32(u32::from_ne_bytes(bytes)).ok_or(wl_display::Error::InvalidMethod.msg(""))
    }
    fn write(&self, data: &mut ThickPtr<u8>, _: &mut ThickPtr<RawFd>) -> Result<()> {
        unsafe {
            data.write_4_bytes(self.to_u32().to_ne_bytes());
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
