use crate::{
    primitives::{read_4_bytes, write_4_bytes},
    wl_display::{self, WlDisplay},
};
use std::{mem::MaybeUninit, os::fd::RawFd};

pub trait Enum: Sized {
    fn from_u32(int: u32) -> Option<Self>;
    fn to_u32(&self) -> u32;

    fn read(data: &mut &[u8], _: &mut &[RawFd]) -> crate::Result<Self, WlDisplay> {
        let bytes = read_4_bytes(data)
            .ok_or(wl_display::Error::InvalidMethod.msg("failed to read enum"))?;

        Self::from_u32(u32::from_ne_bytes(bytes)).ok_or(wl_display::Error::InvalidMethod.msg(""))
    }
    fn write<'o: 'i, 'i>(
        &self,
        data: &'o mut &'i mut [MaybeUninit<u8>],
        _: &'o mut &'i mut [MaybeUninit<RawFd>],
    ) -> crate::Result<(), WlDisplay> {
        write_4_bytes(data, self.to_u32().to_ne_bytes());
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
