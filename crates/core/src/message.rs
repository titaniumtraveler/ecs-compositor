use crate::{Interface, Opcode, Value, object, uint};
use std::os::unix::prelude::RawFd;

pub trait Message<'data>: Value<'data> {
    /// Number of FD args of this message.
    ///
    /// Note: When implementing [`Message`], **don't** set this value manually, but use the generic
    /// constant instead! The reason for this is a weird rust quirk that allows associated
    /// constants in slice types in trait *implementations*, but not in trait *definitions*.
    const FDS: usize;

    type Interface: Interface;
    type Opcode: Opcode;
    const OPCODE: Self::Opcode;
}

#[allow(non_camel_case_types)]
pub struct message_hdr {
    pub object_id: object,
    pub datalen: u16,
    pub opcode: u16,
}

impl Value<'_> for message_hdr {
    fn len(&self) -> u32 {
        4 + 2 + 2
    }

    unsafe fn read(
        data: &mut *const [u8],
        fds: &mut *const [RawFd],
    ) -> crate::primitives::Result<Self> {
        unsafe {
            let object_id = object::read(data, fds)?;
            let i = uint::read(data, fds)?.0;

            let datalen = (i >> 16) as u16;
            let opcode = (i & 0xffff) as u16;

            Ok(Self {
                object_id,
                datalen,
                opcode,
            })
        }
    }

    unsafe fn write(
        &self,
        data: &mut *mut [u8],
        fds: &mut *mut [RawFd],
    ) -> crate::primitives::Result<()> {
        unsafe {
            self.object_id.write(data, fds)?;
            uint((self.datalen as u32) << 16 | self.opcode as u32).write(data, fds)?;
            Ok(())
        }
    }
}

impl message_hdr {
    pub fn content_len(&self) -> u16 {
        self.datalen.wrapping_sub(self.len() as u16)
    }
}
