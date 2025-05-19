use crate::{
    primitives::{Primitive, read_4_bytes, write_4_bytes},
    wl_display::{self, WlDisplay},
};
use std::{mem::MaybeUninit, os::unix::prelude::RawFd};

/// The value is the 32-bit value of the signed int.
pub type Int = i32;

/// The value is the 32-bit value of the unsigned int.
pub type UInt = u32;

impl<'data> Primitive<'data> for Int {
    fn len(&self) -> u32 {
        4
    }

    fn read(data: &mut &'data [u8], _: &mut &[RawFd]) -> crate::Result<Self, WlDisplay> {
        let bytes =
            read_4_bytes(data).ok_or(wl_display::Error::InvalidMethod.msg("failed to read int"))?;

        Ok(Self::from_ne_bytes(bytes))
    }

    fn write<'o: 'i, 'i>(
        &self,
        data: &'o mut &'i mut [MaybeUninit<u8>],
        _: &'o mut &'i mut [MaybeUninit<RawFd>],
    ) -> crate::Result<(), WlDisplay> {
        write_4_bytes(data, self.to_ne_bytes());
        Ok(())
    }
}

impl<'data> Primitive<'data> for UInt {
    fn len(&self) -> u32 {
        4
    }

    fn read(data: &mut &'data [u8], _: &mut &[RawFd]) -> crate::Result<Self, WlDisplay> {
        let bytes = read_4_bytes(data)
            .ok_or(wl_display::Error::InvalidMethod.msg("failed to read uint"))?;

        Ok(Self::from_ne_bytes(bytes))
    }

    fn write<'o: 'i, 'i>(
        &self,
        data: &'o mut &'i mut [MaybeUninit<u8>],
        _: &'o mut &'i mut [MaybeUninit<RawFd>],
    ) -> crate::Result<(), WlDisplay> {
        write_4_bytes(data, self.to_ne_bytes());
        Ok(())
    }
}
