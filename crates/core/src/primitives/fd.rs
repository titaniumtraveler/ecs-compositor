use crate::{
    RawSliceExt,
    primitives::{Primitive, Result},
    wl_display,
};
use std::os::fd::RawFd;

/// The file descriptor is not stored in the message buffer, but in the ancillary data of the UNIX
/// domain socket message (msg_control).
pub struct Fd(pub RawFd);

impl Primitive<'_> for Fd {
    fn len(&self) -> u32 {
        0
    }

    unsafe fn read(_: &mut *const [u8], fds: &mut *const [RawFd]) -> Result<Self> {
        unsafe {
            Ok(Fd(fds
                .split_at(1)
                .ok_or(wl_display::Error::Implementation.msg("not enough fds in read buffer"))?
                .cast::<RawFd>()
                .read()))
        }
    }

    unsafe fn write(&self, _: &mut *mut [u8], fds: &mut *mut [RawFd]) -> Result<()> {
        unsafe {
            fds.split_at(1)
                .ok_or(wl_display::Error::Implementation.msg("fds buffer has not enough space"))?
                .cast::<RawFd>()
                .write(self.0);
        }

        Ok(())
    }
}
