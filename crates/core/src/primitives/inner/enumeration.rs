use crate::{Value, uint};

pub trait enumeration: Value<'static> {
    fn from_u32(int: u32) -> Option<Self>;
    fn to_u32(&self) -> u32;
    fn to_uint(&self) -> uint {
        uint(self.to_u32())
    }
    fn since_version(&self) -> u32;
}

impl enumeration for uint {
    fn from_u32(i: u32) -> Option<Self> {
        Some(uint(i))
    }

    fn to_u32(&self) -> u32 {
        self.0
    }

    fn since_version(&self) -> u32 {
        1
    }
}
