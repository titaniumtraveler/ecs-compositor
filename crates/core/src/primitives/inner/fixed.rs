use crate::{
    RawSliceExt,
    primitives::{Result, Value},
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
pub struct fixed(pub i32);

impl fixed {
    //! Based on `src/wayland-util.h`
    #[inline]
    pub fn to_f64(self) -> f64 {
        f64::from(self.0) / 256.0
    }

    #[inline]
    pub fn from_f64(d: f64) -> Self {
        fixed(d as i32)
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

impl Value<'_> for fixed {
    fn len(&self) -> u32 {
        4
    }

    unsafe fn read(data: &mut *const [u8], _: &mut *const [RawFd]) -> Result<Self> {
        let i32 = unsafe {
            data.split_at(4)
                .ok_or(wl_display::Error::invalid_method.msg("failed to read fixed-point"))?
                .cast::<i32>()
                .read()
        };

        Ok(fixed(i32))
    }

    unsafe fn write<'a>(&self, data: &mut *mut [u8], _: &mut *mut [RawFd]) -> Result<()> {
        unsafe {
            data.split_at(4)
                .ok_or(wl_display::Error::implementation.msg("not enough buffer space"))?
                .cast::<i32>()
                .write(self.0);
        }
        Ok(())
    }
}
