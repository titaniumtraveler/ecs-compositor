use crate::{
    primitives::{Primitive, Result, ThickPtr},
    wl_display,
};
use std::os::fd::{AsRawFd, BorrowedFd, RawFd};

/// The file descriptor is not stored in the message buffer, but in the ancillary data of the UNIX
/// domain socket message (msg_control).
pub struct Fd(pub RawFd);

impl Primitive<'_> for Fd {
    fn len(&self) -> u32 {
        0
    }

    fn read(_: &mut &[u8], fds: &mut &[RawFd]) -> Result<Self> {
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

    fn write<'a>(&self, _: &mut ThickPtr<u8>, fds: &mut ThickPtr<RawFd>) -> Result<()> {
        debug_assert!(fds.len == 0, "fds is empty");

        // SAFETY: `self` has to be a valid file descriptor
        let dupped = unsafe { BorrowedFd::borrow_raw(self.0) }
            .try_clone_to_owned()
            .map_err(|_| wl_display::Error::Implementation.msg("failed to duplicate fd"))?;

        unsafe {
            fds.write(dupped.as_raw_fd());
        }

        Ok(())
    }
}
