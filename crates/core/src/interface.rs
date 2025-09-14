use crate::{primitives::enumeration, uint};

pub trait Interface {
    const NAME: &str;
    const VERSION: u32;

    type Error: enumeration;

    type Request: Opcode;
    type Event: Opcode;
}

/// Interface for [`new_id`]/[`object`] without a specific interface set.
///
/// [`new_id`]: crate::primitives::new_id
/// [`object`]: crate::primitives::object
impl Interface for () {
    const NAME: &str = "";
    const VERSION: u32 = 0;

    type Error = uint;

    type Request = u16;
    type Event = u16;
}

pub trait Opcode: Sized {
    fn from_u16(i: u16) -> Result<Self, u16>;
    fn to_u16(self) -> u16;
}

impl Opcode for u16 {
    fn from_u16(i: u16) -> Result<Self, u16> {
        Ok(i)
    }

    fn to_u16(self) -> u16 {
        self
    }
}
