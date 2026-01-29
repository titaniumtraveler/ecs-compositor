use crate::protocols::wayland::wl_registry;
use ecs_compositor_core::{Interface, Message, RawSliceExt, Value, new_id, primitives::align, uint};
use std::{fmt::Display, os::fd::RawFd};
use tracing::debug;

#[allow(non_camel_case_types)]
pub struct bind<I: Interface> {
    pub name: uint,
    pub id: new_id<I>,
}

pub struct BindBuilder {
    name: uint,
    id: new_id,
}

impl wl_registry::event::global<'_> {
    pub fn bind(self, obj: &mut Option<(uint, uint)>) {
        debug!(event = %self,"received global");
        let wl_registry::event::global { name, version, .. } = self;
        *obj = Some((name, version));
    }
}

impl BindBuilder {
    pub fn build<I: Interface>(self) -> bind<I> {
        bind { name: self.name, id: self.id.cast() }
    }
}

impl<'data, I: Interface> Value<'data> for bind<I> {
    const FDS: usize = 0;

    fn len(&self) -> u32 {
        4 // self.name
        + 4 + align::<4>(I::NAME.len() as u32 + 1) // Interface::NAME
        + 4 // Interface::VERSION
        + 4 // self.id
    }

    unsafe fn read(
        _data: &mut *const [u8],
        _fds: &mut *const [RawFd],
    ) -> ecs_compositor_core::primitives::Result<Self> {
        unimplemented!()
    }

    unsafe fn write(
        &self,
        data: &mut *mut [u8],
        fds: &mut *mut [RawFd],
    ) -> ecs_compositor_core::primitives::Result<()> {
        unsafe {
            self.name.write(data, fds)?;

            str_with_nul(I::NAME).write(data, fds)?;
            uint(I::VERSION).write(data, fds)?;

            self.id.write(data, fds)?;
            Ok(())
        }
    }
}

impl<I: Interface> Display for bind<I> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "new_id_dyn{{ name: {}, id: {}, version: {}}}",
            self.name,
            self.id,
            I::VERSION
        )?;
        Ok(())
    }
}

impl<'data, I: Interface> Message<'data> for bind<I> {
    type Interface = wl_registry::wl_registry;

    const VERSION: u32 = wl_registry::request::bind::VERSION;
    const NAME: &'static str = wl_registry::request::bind::NAME;

    type Opcode = <wl_registry::request::bind<'data> as Message<'data>>::Opcode;

    const OPCODE: Self::Opcode = <wl_registry::request::bind<'data> as Message<'data>>::OPCODE;
    const OP: u16 = <wl_registry::request::bind<'data> as Message<'data>>::OP;
}

#[allow(non_camel_case_types)]
pub struct str_with_nul<'data>(pub &'data str);

impl Value<'_> for str_with_nul<'_> {
    const FDS: usize = 0;

    fn len(&self) -> u32 {
        4 + align::<4>((self.0.len() as u32) + 1)
    }

    unsafe fn read(_: &mut *const [u8], _: &mut *const [RawFd]) -> ecs_compositor_core::primitives::Result<Self> {
        unimplemented!()
    }

    unsafe fn write(
        &self,
        data: &mut *mut [u8],
        fds: &mut *mut [RawFd],
    ) -> ecs_compositor_core::primitives::Result<()> {
        unsafe {
            let str = self.0;

            // Write the &str to the buffer.
            // Because the it lacks the expected null terminator,
            // we just pretend we write a string with len+1 to the buffer and then set the
            // padding (which is there *anyways*) to zero, which makes sure we the string data
            // is followed by a null byte. (Which has effectively the same impact as if we
            // wrote a full null terminated string)
            let str_len = str.len() as u32 + 1;
            uint(str_len).write(data, fds)?;
            let (padding, data) = {
                let mut padding = data
                    .split_at(align::<4>(str_len) as usize)
                    .expect("not enough space for string");
                let data = padding.split_at(str.len()).unwrap();
                (padding, data)
            };

            data.start().copy_from_nonoverlapping(str.as_ptr(), str.len());
            padding.start().write_bytes(0, padding.len());
            Ok(())
        }
    }
}
