use crate::primitives::Enum;

pub trait Interface {
    const NAME: &str;
    const VERSION: u32;

    type Error: Enum;
}

/// Interface for [`NewId`]/[`Object`] without a specific interface set.
impl Interface for () {
    const NAME: &str = "";
    const VERSION: u32 = 0;

    type Error = u32;
}
