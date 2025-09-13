pub use self::{
    error::*,
    interface::Interface,
    primitives::Primitive,
    primitives::{Array, Enum, Fd, Fixed, Int, NewId, NewIdDyn, Object, String, UInt},
    raw_slice::RawSliceExt,
};
use std::os::fd::RawFd;

pub mod error;
pub mod interface;
pub mod primitives;
mod raw_slice;
pub mod wl_display;

#[allow(clippy::len_without_is_empty)] // Again clippy! We are not a collection!
pub trait Message<'data, const FDS: usize, I: Interface>: Sized {
    /// Number of FD args of this message.
    ///
    /// Note: When implementing [`Message`], **don't** set this value manually, but use the generic
    /// constant instead! The reason for this is a weird rust quirk that allows associated
    /// constants in slice types in trait *implementations*, but not in trait *definitions*.
    const FDS: usize = FDS;

    fn len(&self) -> u32;

    /// Reads message from queue.
    fn read(data: &'data [u8], fds: &[RawFd; FDS]) -> primitives::Result<Self>;
    /// Writes the message to queue
    /// `data.len()` is guarantied by the caller to be the same size the return value of [`Self::write_len()`]
    fn write(&self, data: &mut *mut [u8], fds: &mut *mut [RawFd]) -> primitives::Result<()>;
}
