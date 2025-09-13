use crate::{UInt, Value};

pub trait Enum: Value<'static> {
    fn from_u32(int: u32) -> Option<Self>;
    fn to_u32(&self) -> u32;
}

impl Enum for UInt {
    fn from_u32(uint: u32) -> Option<Self> {
        Some(UInt(uint))
    }

    fn to_u32(&self) -> u32 {
        self.0
    }
}
