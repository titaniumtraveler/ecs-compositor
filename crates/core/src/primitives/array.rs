use crate::{
    primitives::{Primitive, Result, ThickPtr, read_4_bytes},
    wl_display,
};
use std::{
    marker::PhantomData,
    num::NonZero,
    os::unix::prelude::RawFd,
    ptr::{self, NonNull},
};

/// Starts with 32-bit array size in bytes, followed by the array contents verbatim, and finally
/// padding to a 32-bit boundary.
#[derive(Debug)]
pub struct Array<'a> {
    /// If this is set to [`None`], this implies that the data has already been written to the
    /// buffer, which means only the header has to be set.
    pub ptr: Option<NonNull<u8>>,
    /// Note that this length isn't the size of the allocation, but the size if the *data*, which
    /// means after `ptr + len` there might be `0..=3` bytes of padding.
    pub len: u32,
    _marker: PhantomData<&'a [u8]>,
}

impl<'data> Primitive<'data> for Array<'data> {
    #[inline]
    fn len(&self) -> u32 {
        4 + pad_to_4(self.len)
    }

    #[inline]
    fn read(data: &mut &'data [u8], _: &mut &[RawFd]) -> Result<Self> {
        let (ptr, len) =
            read_data(data).ok_or(wl_display::Error::InvalidMethod.msg("failed to read array"))?;

        Ok(Self {
            ptr: NonNull::new(ptr.as_ptr() as *mut _),
            len,
            _marker: PhantomData,
        })
    }

    #[inline]
    fn write(&self, data: &mut ThickPtr<u8>, _: &mut ThickPtr<RawFd>) -> Result<()> {
        write_data(self.ptr, self.len, data);
        Ok(())
    }
}

/// Starts with an unsigned 32-bit length (including null terminator), followed by the string
/// contents, including terminating null byte, then padding to a 32-bit boundary. A null value is
/// represented with a length of 0.
pub struct String<'a> {
    pub ptr: Option<NonNull<u8>>,
    pub len: NonZero<u32>,
    _marker: PhantomData<&'a [u8]>,
}

impl<'data> Primitive<'data> for String<'data> {
    #[inline]
    fn len(&self) -> u32 {
        let header = u32::BITS / 8;
        header + pad_to_4(self.len.into())
    }

    #[inline]
    fn read(data: &mut &'data [u8], _: &mut &[RawFd]) -> Result<Self> {
        let (ptr, len) =
            read_data(data).ok_or(wl_display::Error::InvalidMethod.msg("failed to read string"))?;

        match NonZero::new(len) {
            Some(len) => Ok(String {
                ptr: NonNull::new(ptr.as_ptr() as *mut u8),
                len,
                _marker: PhantomData,
            }),
            None => Err(wl_display::Error::InvalidMethod.msg("empty string not allowed here")),
        }
    }

    #[inline]
    fn write(&self, data: &mut ThickPtr<u8>, _: &mut ThickPtr<RawFd>) -> Result<()> {
        write_data(self.ptr, self.len.get(), data);
        Ok(())
    }
}

impl<'data> Primitive<'data> for Option<String<'data>> {
    #[inline]
    fn len(&self) -> u32 {
        4 + self.as_ref().map(String::len).unwrap_or(0)
    }

    #[inline]
    fn read(data: &mut &'data [u8], _: &mut &[RawFd]) -> Result<Self> {
        let (ptr, len) =
            read_data(data).ok_or(wl_display::Error::InvalidMethod.msg("failed to read string"))?;

        match NonZero::new(len) {
            None => Ok(None),
            Some(len) => Ok(Some(String {
                ptr: NonNull::new(ptr.as_ptr() as *mut u8),
                len,
                _marker: PhantomData,
            })),
        }
    }

    #[inline]
    fn write(&self, data: &mut ThickPtr<u8>, _: &mut ThickPtr<RawFd>) -> Result<()> {
        let (src, len) = match self {
            Some(string) => (string.ptr, string.len.get()),
            None => (None, 0),
        };
        write_data(src, len, data);
        Ok(())
    }
}

const fn pad_to_4(x: u32) -> u32 {
    const fn pad_to<const ALIGN: u32>(x: u32) -> u32 {
        ((x + (ALIGN - 1)) & !(ALIGN - 1)) - x
    }

    pad_to::<4>(x)
}

#[allow(clippy::identity_op)]
#[test]
fn test_align_to_4() {
    for i in 0..=16 {
        assert_eq!(pad_to_4(i * 4 + 0), 0);
        assert_eq!(pad_to_4(i * 4 + 1), 3);
        assert_eq!(pad_to_4(i * 4 + 2), 2);
        assert_eq!(pad_to_4(i * 4 + 3), 1);
    }
}

#[inline]
fn read_data<'data>(data: &mut &'data [u8]) -> Option<(&'data [u8], u32)> {
    let [a, b, c, d] = read_4_bytes(data)?;
    let len = u32::from_ne_bytes([a, b, c, d]);

    let (str, tail) = data.split_at_checked((len + pad_to_4(len)) as usize)?;
    *data = tail;

    Some((str, len))
}

#[inline]
fn write_data(src: Option<NonNull<u8>>, len: u32, data: &mut ThickPtr<u8>) {
    unsafe {
        // Write len header
        data.write_slice(&len.to_ne_bytes());

        let Some(src) = src else {
            // The actual data was already written, so only write the header
            data.advance((len + pad_to_4(len)) as usize);

            return;
        };

        // SAFETY: `self.ptr` + `self.len` are guarantied to point to valid data.
        let src = &*ptr::slice_from_raw_parts(src.as_ptr(), len as usize);
        data.write_slice(src);

        // Explicitly zero out the padding bytes.
        data.write_zeros(pad_to_4(len) as usize);
    }
}
