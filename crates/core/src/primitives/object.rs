use crate::{
    Error, Interface, RawSliceExt, String, UInt,
    primitives::{Primitive, Result},
    wl_display,
};
use std::{marker::PhantomData, num::NonZero, os::unix::prelude::RawFd};

/// 32-bit object ID.
/// A null value is represented with an ID of 0.
///
/// Note that the Rust impl uses [`Option<Object<Object>>`] instead.
/// (And makes sure to provide a niche using [`NonZero<u32>`] to make sure that doesn't have any
/// runtime impact)
#[derive(Debug, Clone, Copy)]
pub struct Object<I: Interface = ()> {
    id: NonZero<u32>,
    _marker: PhantomData<I>,
}

impl<I: Interface> Object<I> {
    pub const fn from_id(id: NonZero<u32>) -> Self {
        Self {
            id,
            _marker: PhantomData,
        }
    }

    pub const fn cast<To: Interface>(self) -> Object<To> {
        let Object { id, _marker: _ } = self;

        Object {
            id,
            _marker: PhantomData,
        }
    }

    pub fn id(&self) -> NonZero<u32> {
        self.id
    }

    pub fn err(self, err: I::Error, msg: &'static str) -> Error<I> {
        Error::new(self, err, msg)
    }
}

impl<I: Interface> Primitive<'_> for Object<I> {
    fn len(&self) -> u32 {
        4
    }

    unsafe fn read(data: &mut *const [u8], _: &mut *const [RawFd]) -> Result<Self> {
        let id = unsafe { read_id(data)? }
            .ok_or(wl_display::Error::InvalidMethod.msg("null object not allowed here"))?;

        Ok(Self {
            id,
            _marker: PhantomData,
        })
    }

    unsafe fn write<'a>(&self, data: &mut *mut [u8], _: &mut *mut [RawFd]) -> Result<()> {
        unsafe { write_id(data, self.id.get())? }
        Ok(())
    }
}

impl<I: Interface> Primitive<'_> for Option<Object<I>> {
    fn len(&self) -> u32 {
        4
    }

    unsafe fn read(data: &mut *const [u8], _: &mut *const [RawFd]) -> Result<Self> {
        match unsafe { read_id(data)? } {
            None => Ok(None),
            Some(id) => Ok(Some(Object {
                id,
                _marker: PhantomData,
            })),
        }
    }

    unsafe fn write<'a>(&self, data: &mut *mut [u8], _: &mut *mut [RawFd]) -> Result<()> {
        unsafe {
            write_id(
                data,
                self.as_ref().map(|object| object.id.get()).unwrap_or(0),
            )?;
        }
        Ok(())
    }
}

/// The 32-bit object ID. Generally, the interface used for the new object is inferred from the
/// xml, but in the case where it's not specified, a new_id is preceded by a string specifying the
/// interface name, and a uint specifying the version.
pub struct NewId<I: Interface = ()> {
    pub id: NonZero<u32>,
    pub _marker: PhantomData<I>,
}

impl<I: Interface> NewId<I> {
    pub fn cast<To: Interface>(self) -> NewId<To> {
        let NewId { id, _marker: _ } = self;

        NewId {
            id,
            _marker: PhantomData,
        }
    }

    pub fn id(&self) -> NonZero<u32> {
        self.id
    }

    pub fn to_object(&self) -> Object<I> {
        Object {
            id: self.id,
            _marker: self._marker,
        }
    }

    pub fn err(self, err: I::Error, msg: &'static str) -> Error<I> {
        Error::new(self.to_object(), err, msg)
    }
}

impl<I: Interface> Primitive<'_> for NewId<I> {
    fn len(&self) -> u32 {
        4
    }

    unsafe fn read(data: &mut *const [u8], _: &mut *const [RawFd]) -> Result<Self> {
        Ok(NewId {
            id: unsafe {
                read_id(data)?.ok_or(
                    wl_display::Error::Implementation.msg("id with value 0 is not allowed here"),
                )?
            },
            _marker: PhantomData,
        })
    }

    unsafe fn write<'a>(&self, data: &mut *mut [u8], _: &mut *mut [RawFd]) -> Result<()> {
        unsafe { write_id(data, self.id.get())? }
        Ok(())
    }
}

pub struct NewIdDyn<'data> {
    pub name: String<'data>,
    pub version: UInt,
    pub id: NewId,
}

impl<'data> Primitive<'data> for NewIdDyn<'data> {
    fn len(&self) -> u32 {
        self.name.len() + self.version.len() + self.id.len()
    }

    unsafe fn read(data: &mut *const [u8], fds: &mut *const [RawFd]) -> Result<Self> {
        unsafe {
            Ok(Self {
                name: String::read(data, fds)?,
                version: UInt::read(data, fds)?,
                id: NewId::read(data, fds)?,
            })
        }
    }

    unsafe fn write(&self, data: &mut *mut [u8], fds: &mut *mut [RawFd]) -> Result<()> {
        unsafe {
            self.name.write(data, fds)?;
            self.version.write(data, fds)?;
            self.id.write(data, fds)?;
        }
        Ok(())
    }
}

unsafe fn read_id(data: &mut *const [u8]) -> Result<Option<NonZero<u32>>> {
    let u32 = unsafe {
        data.split_at(4)
            .ok_or(wl_display::Error::InvalidMethod.msg("failed to read object id"))?
            .cast::<u32>()
            .read()
    };

    Ok(NonZero::new(u32))
}

unsafe fn write_id(data: &mut *mut [u8], id: u32) -> Result<()> {
    unsafe {
        data.split_at(4)
            .ok_or(wl_display::Error::Implementation.msg("not enough write buffer space"))?
            .cast::<u32>()
            .write(id);
    }
    Ok(())
}
