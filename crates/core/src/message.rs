use crate::{Interface, Opcode, Value, object, uint};
use std::os::unix::prelude::RawFd;

pub trait Message<'data>: Value<'data> {
    type Interface: Interface;
    const VERSION: u32;
    const NAME: &'static str;

    type Opcode: Opcode;
    const OPCODE: Self::Opcode;
    const OP: u16;
}

#[derive(Debug, Clone, Copy)]
#[allow(non_camel_case_types)]
pub struct message_header {
    pub object_id: object,
    pub datalen: u16,
    pub opcode: u16,
}

impl Value<'_> for message_header {
    const FDS: usize = 0;
    fn len(&self) -> u32 {
        Self::DATA_LEN as u32
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

impl message_header {
    pub const DATA_LEN: u16 = 4 + 2 + 2;
    pub const CTRL_LEN: usize = 0;

    pub const COMBINED_LEN: (u16, usize) = (Self::DATA_LEN, Self::CTRL_LEN);

    pub fn content_len(&self) -> u16 {
        self.datalen.wrapping_sub(self.len() as u16)
    }
}
