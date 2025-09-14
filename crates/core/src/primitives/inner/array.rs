use crate::{
    RawSliceExt,
    primitives::{Result, Value, align},
    wl_display::enumeration::error,
};
use std::{marker::PhantomData, num::NonZero, os::unix::prelude::RawFd, ptr::NonNull};

/// Starts with 32-bit array size in bytes, followed by the array contents verbatim, and finally
/// padding to a 32-bit boundary.
#[derive(Debug)]
pub struct array<'a> {
    /// If this is set to [`None`], this implies that the data has already been written to the
    /// buffer, which means only the header has to be set.
    pub ptr: Option<NonNull<u8>>,
    /// Note that this length isn't the size of the allocation, but the size if the *data*, which
    /// means after `ptr + len` there might be `0..=3` bytes of padding.
    pub len: u32,
    _marker: PhantomData<&'a [u8]>,
}

impl<'data> Value<'data> for array<'data> {
    #[inline]
    fn len(&self) -> u32 {
        4 + align::<4>(self.len)
    }

    #[inline]
    unsafe fn read(data: &mut *const [u8], _: &mut *const [RawFd]) -> Result<Self> {
        unsafe {
            let (ptr, len) = read(data)?;

            Ok(Self {
                ptr: Some(ptr),
                len,
                _marker: PhantomData,
            })
        }
    }

    #[inline]
    unsafe fn write(&self, data: &mut *mut [u8], _: &mut *mut [RawFd]) -> Result<()> {
        unsafe { write(data, self.ptr, self.len) }
    }
}

/// Starts with an unsigned 32-bit length (including null terminator), followed by the string
/// contents, including terminating null byte, then padding to a 32-bit boundary. A null value is
/// represented with a length of 0. (In Rust as `Option::<String>::None`)
pub struct string<'a> {
    pub ptr: Option<NonNull<u8>>,
    pub len: NonZero<u32>,
    _marker: PhantomData<&'a [u8]>,
}

impl<'data> Value<'data> for string<'data> {
    #[inline]
    fn len(&self) -> u32 {
        4 + align::<4>(self.len.get())
    }

    #[inline]
    unsafe fn read(data: &mut *const [u8], _: &mut *const [RawFd]) -> Result<Self> {
        let (ptr, len) = unsafe { read(data) }?;

        Ok(string {
            ptr: Some(ptr),
            len: NonZero::new(len)
                .ok_or(error::invalid_method.msg("empty string not allowed here"))?,
            _marker: PhantomData,
        })
    }

    #[inline]
    unsafe fn write(&self, data: &mut *mut [u8], _: &mut *mut [RawFd]) -> Result<()> {
        unsafe { write(data, self.ptr, self.len.get()) }
    }
}

impl<'data> Value<'data> for Option<string<'data>> {
    #[inline]
    fn len(&self) -> u32 {
        4 + self.as_ref().map(string::len).unwrap_or(0)
    }

    #[inline]
    unsafe fn read(data: &mut *const [u8], _: &mut *const [RawFd]) -> Result<Self> {
        let (ptr, len) = unsafe { read(data) }?;

        Ok(NonZero::new(len).map(|len| string {
            ptr: Some(ptr),
            len,
            _marker: PhantomData,
        }))
    }

    #[inline]
    unsafe fn write(&self, data: &mut *mut [u8], _: &mut *mut [RawFd]) -> Result<()> {
        unsafe {
            write(
                data,
                self.as_ref().map(|str| str.ptr).unwrap_or(None),
                self.as_ref().map(|str| str.len.get()).unwrap_or(0),
            )
        }
    }
}

#[allow(clippy::manual_inspect)]
#[inline]
pub unsafe fn read(data: &mut *const [u8]) -> Result<(NonNull<u8>, u32)> {
    let old = *data;
    (|| unsafe {
        let len = {
            let len = data
                .split_at(4)
                .ok_or_else(|| error::implementation.msg("reading buffer too short"))?
                .cast::<u32>();
            debug_assert!(len.is_aligned());
            len.read()
        };

        let content = data.split_at(align::<4>(len) as usize).ok_or_else(|| {
            error::implementation.msg("reading buffer too short for message content")
        })?;

        // Safety: `data` is guarantied by caller to point to a valid buffer.
        Ok((NonNull::new_unchecked(content as *mut u8), len))
    })()
    .map_err(|err| {
        *data = old;
        err
    })
}

/// Write [`String`]/[`Array`] data.
///
/// If there is not enough room on the buffer, throws an error.
/// If `ptr` is `None`, only writes the header and assumes the user has already written the actual
/// content (including the padding bytes to the 4 byte boundary).
///
/// # Safety
///
/// - `data` has to point to a valid buffer **and** has to be aligned to a 4 byte boundary.
///   See [`crate::primitives::align()`] as an helper.
/// - `ptr` if `Some(_)` has to be valid for `len` bytes.
#[inline]
pub unsafe fn write(data: &mut *mut [u8], ptr: Option<NonNull<u8>>, len: u32) -> Result<()> {
    unsafe {
        // Check if the buffer has at least header + data space.
        let padded_len = align::<4>(len);
        if data.len() < 4 + padded_len as usize {
            return Err(error::implementation.msg("not enough buffer provided"));
        }

        let len_hdr = data.split_at_unchecked(4).cast::<u32>();
        debug_assert!(len_hdr.is_aligned());
        len_hdr.write(len);

        let (content, padding) = {
            let mut content = data.split_at_unchecked(align::<4>(len) as usize);
            (
                content.split_at_unchecked(align::<4>(len) as usize),
                content,
            )
        };
        if let Some(ptr) = ptr {
            content
                .cast::<u8>()
                .copy_from_nonoverlapping(ptr.as_ptr(), len as usize);

            padding.cast::<u8>().write_bytes(0, padding.len());
        }

        Ok(())
    }
}
