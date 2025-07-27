use crate::{
    Error, Interface, String, UInt,
    primitives::{Primitive, Result, ThickPtr, read_4_bytes},
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

    fn read(data: &mut &[u8], _: &mut &[RawFd]) -> Result<Self> {
        let id = read_id(data)?
            .ok_or(wl_display::Error::InvalidMethod.msg("null object not allowed here"))?;

        Ok(Self {
            id,
            _marker: PhantomData,
        })
    }

    fn write<'a>(&self, data: &mut ThickPtr<u8>, _: &mut ThickPtr<RawFd>) -> Result<()> {
        unsafe {
            data.write_4_bytes(self.id.get().to_ne_bytes());
        }
        Ok(())
    }
}

impl<I: Interface> Primitive<'_> for Option<Object<I>> {
    fn len(&self) -> u32 {
        4
    }

    fn read(data: &mut &[u8], _: &mut &[RawFd]) -> Result<Self> {
        match read_id(data)? {
            None => Ok(None),
            Some(id) => Ok(Some(Object {
                id,
                _marker: PhantomData,
            })),
        }
    }

    fn write<'a>(&self, data: &mut ThickPtr<u8>, _: &mut ThickPtr<RawFd>) -> Result<()> {
        let id = self.as_ref().map(|object| object.id.get()).unwrap_or(0);
        unsafe {
            data.write_4_bytes(id.to_ne_bytes());
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

    fn read(data: &mut &'_ [u8], _: &mut &[RawFd]) -> Result<Self> {
        let id = read_id(data)?
            .ok_or(wl_display::Error::InvalidMethod.msg("new_id is not allowed to be 0"))?;

        Ok(NewId {
            id,
            _marker: PhantomData,
        })
    }

    fn write<'a>(&self, data: &mut ThickPtr<u8>, _: &mut ThickPtr<RawFd>) -> Result<()> {
        unsafe {
            data.write_4_bytes(self.id.get().to_ne_bytes());
        }
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

    fn read(data: &mut &'data [u8], fds: &mut &[RawFd]) -> Result<Self> {
        Ok(Self {
            name: String::read(data, fds)?,
            version: UInt::read(data, fds)?,
            id: NewId::read(data, fds)?,
        })
    }

    fn write(&self, data: &mut ThickPtr<u8>, fds: &mut ThickPtr<RawFd>) -> Result<()> {
        self.name.write(data, fds)?;
        self.version.write(data, fds)?;
        self.id.write(data, fds)?;
        Ok(())
    }
}

fn read_id(data: &mut &[u8]) -> Result<Option<NonZero<u32>>> {
    let bytes = read_4_bytes(data)
        .ok_or(wl_display::Error::InvalidMethod.msg("failed to read object id"))?;

    Ok(NonZero::new(u32::from_ne_bytes(bytes)))
}
