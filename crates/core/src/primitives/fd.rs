use crate::{
    Result,
    primitives::Primitive,
    wl_display::{self, WlDisplay},
};
use std::{
    mem::MaybeUninit,
    os::fd::{AsRawFd, BorrowedFd, RawFd},
};

/// The file descriptor is not stored in the message buffer, but in the ancillary data of the UNIX
/// domain socket message (msg_control).
pub struct Fd(pub RawFd);

impl Primitive<'_> for Fd {
    fn len(&self) -> u32 {
        0
    }

    fn read(_: &mut &[u8], fds: &mut &[RawFd]) -> crate::Result<Self, WlDisplay> {
        match fds[..] {
            [fd, ref tail @ ..] => {
                *fds = tail;

                Ok(Fd(fd))
            }
            [] => {
                panic!("fds is empty")
            }
        }
    }

    fn write<'o: 'i, 'i>(
        &self,
        _: &'o mut &'i mut [MaybeUninit<u8>],
        fds: &'o mut &'i mut [MaybeUninit<RawFd>],
    ) -> Result<(), WlDisplay> {
        match fds[..] {
            [ref mut fd, ref mut tail @ ..] => {
                *fds = tail;
                // SAFETY: `self` has to be a valid file descriptor
                let dupped = unsafe { BorrowedFd::borrow_raw(self.0) }
                    .try_clone_to_owned()
                    .map_err(|_| wl_display::Error::Implementation.msg("failed to duplicate fd"))?;

                fd.write(dupped.as_raw_fd());

                Ok(())
            }
            [] => {
                panic!("fds is empty")
            }
        }
    }
}
