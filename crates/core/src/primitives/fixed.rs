use crate::{
    primitives::{Primitive, Result, ThickPtr, read_4_bytes},
    wl_display,
};
use std::os::unix::prelude::RawFd;

/// Fixed-point number
///
/// A [`Fixed`] is a 24.8 signed fixed-point number with a sign bit, 23 bits
/// of integer precision and 8 bits of decimal precision. Consider [`Fixed`]
/// as an opaque struct with methods that facilitate conversion to and from
/// [`f64`] and [`i32`] types.
#[allow(non_camel_case_types)]
pub struct Fixed(pub i32);

impl Fixed {
    //! Based on `src/wayland-util.h`
    #[inline]
    pub fn to_f64(self) -> f64 {
        f64::from(self.0) / 256.0
    }

    #[inline]
    pub fn from_f64(d: f64) -> Self {
        Fixed(d as i32)
    }

    #[inline]
    pub fn to_i32(self) -> i32 {
        self.0 / 256
    }

    #[inline]
    pub fn from_i32(i: i32) -> Self {
        Self(i * 256)
    }
}

impl Primitive<'_> for Fixed {
    fn len(&self) -> u32 {
        4
    }

    fn read(data: &mut &'_ [u8], _: &mut &[RawFd]) -> Result<Self> {
        let bytes = read_4_bytes(data)
            .ok_or(wl_display::Error::InvalidMethod.msg("failed to read fixed-point"))?;

        Ok(Fixed(i32::from_ne_bytes(bytes)))
    }

    fn write<'a>(&self, data: &mut ThickPtr<u8>, _: &mut ThickPtr<RawFd>) -> Result<()> {
        unsafe {
            data.write_4_bytes(self.0.to_ne_bytes());
        }
        Ok(())
    }
}
