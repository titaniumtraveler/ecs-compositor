pub use self::{
    error::*,
    interface::Interface,
    primitives::Value,
    primitives::{Array, Enum, Fd, Fixed, Int, NewId, NewIdDyn, Object, String, UInt},
    raw_slice::RawSliceExt,
};

pub mod error;
pub mod interface;
pub mod primitives;
mod raw_slice;
pub mod wl_display;

#[allow(clippy::len_without_is_empty)] // Again clippy! We are not a collection!
pub trait Message<'data, const FDS: usize, I: Interface>: Value<'data> {
    /// Number of FD args of this message.
    ///
    /// Note: When implementing [`Message`], **don't** set this value manually, but use the generic
    /// constant instead! The reason for this is a weird rust quirk that allows associated
    /// constants in slice types in trait *implementations*, but not in trait *definitions*.
    const FDS: usize = FDS;
}
