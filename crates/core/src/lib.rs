use std::{mem::MaybeUninit, os::fd::RawFd};

pub use self::{
    error::*,
    interface::Interface,
    primitives::{
        Array as WlArray, Enum as WlEnum, Fixed as WlFixed, Int as WlInt, NewId as WlNewId,
        Object as WlObject, String as WlString, UInt as WlUInt,
    },
};

pub mod error;
pub mod interface;
pub mod primitives;
pub mod wl_display;

pub trait Message<const FDS: usize, I: Interface>: Sized {
    /// Number of FD args of this message.
    ///
    /// Note: When implementing [`Message`], **don't** set this value manually, but use the generic
    /// constant instead! The reason for this is a weird rust quirk that allows associated
    /// constants in slice types in trait *implementations*, but not in trait *definitions*.
    const FDS: usize;

    /// Reads message from queue.
    fn read(data: &[u8], fds: &[RawFd; FDS]) -> crate::Result<Self, I>;
    fn write_len(&self) -> usize;

    /// Writes the message to queue
    /// `data.len()` is guarantied by the caller to be the same size the return value of [`Self::write_len()`]
    fn write(
        &self,
        data: &mut [MaybeUninit<u8>],
        fds: &mut [MaybeUninit<RawFd>; FDS],
    ) -> crate::Result<(), I>;
}
