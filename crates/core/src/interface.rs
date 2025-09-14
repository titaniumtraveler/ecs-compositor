use crate::primitives::*;

pub trait Interface {
    const NAME: &str;
    const VERSION: u32;

    type Error: enumeration;
}

/// Interface for [`NewId`]/[`Object`] without a specific interface set.
impl Interface for () {
    const NAME: &str = "";
    const VERSION: u32 = 0;

    type Error = uint;
}
