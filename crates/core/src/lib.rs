use std::os::fd::RawFd;

pub use self::{
    error::*,
    interface::Interface,
    primitives::Primitive,
    primitives::{Array, Enum, Fd, Fixed, Int, NewId, NewIdDyn, Object, String, ThickPtr, UInt},
};

pub mod error;
pub mod interface;
pub mod primitives;
pub mod wl_display;

pub trait Message<'data, const FDS: usize, I: Interface>: Sized {
    /// Number of FD args of this message.
    ///
    /// Note: When implementing [`Message`], **don't** set this value manually, but use the generic
    /// constant instead! The reason for this is a weird rust quirk that allows associated
    /// constants in slice types in trait *implementations*, but not in trait *definitions*.
    const FDS: usize = FDS;

    /// Reads message from queue.
    fn read(data: &'data [u8], fds: &[RawFd; FDS]) -> primitives::Result<Self>;
    fn write_len(&self) -> u32;

    /// Writes the message to queue
    /// `data.len()` is guarantied by the caller to be the same size the return value of [`Self::write_len()`]
    fn write(&self, data: &mut ThickPtr<u8>, fds: &mut ThickPtr<RawFd>) -> primitives::Result<()>;
}
