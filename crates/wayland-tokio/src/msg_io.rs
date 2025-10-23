#![allow(dead_code)]

use bstr::ByteSlice;
use ecs_compositor_core::RawSliceExt;
use libc::{__errno_location, c_int, iovec, msghdr, ssize_t};
use std::{fmt::Debug, mem::MaybeUninit, os::fd::RawFd, ptr::null_mut, slice};
use tracing::{instrument, trace};

pub mod cmsg_cursor;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Msg {
    pub data: *mut [u8],
    pub ctrl: *mut [u8],
    pub flags: c_int,
}

impl Msg {
    fn init_msghdr<'a>(
        self,
        hdr: &'a mut MaybeUninit<msghdr>,
        iovec: &'a mut MaybeUninit<iovec>,
    ) -> &'a mut msghdr {
        unsafe {
            let iovec = iovec.write(iovec {
                iov_base: self.data.start().cast(),
                iov_len: self.data.len(),
            });

            hdr.write(msghdr {
                msg_name: null_mut(),
                msg_namelen: 0,
                msg_iov: iovec,
                msg_iovlen: 1,
                msg_control: self.ctrl.start().cast(),
                msg_controllen: self.ctrl.len(),
                msg_flags: self.flags,
            })
        }
    }

    pub fn recv(&mut self, socket: RawFd, flags: c_int) -> Result<Option<Self>, c_int> {
        unsafe {
            let mut iovec = iovec {
                iov_base: self.data.start().cast(),
                iov_len: self.data.len(),
            };

            let mut msg = msghdr {
                msg_name: null_mut(),
                msg_namelen: 0,
                msg_iov: &mut iovec,
                msg_iovlen: 1,
                msg_control: self.ctrl.start().cast(),
                msg_controllen: self.ctrl.len(),
                msg_flags: self.flags,
            };

            let res = libc::recvmsg(socket, &mut msg, flags);
            self.handle_res(socket, &msg, flags, res)
        }
    }

    #[instrument(name = "sendmsg", level = "trace", ret, skip_all)]
    pub fn send(&mut self, socket: RawFd, flags: c_int) -> Result<Option<Msg>, c_int> {
        unsafe {
            let mut iovec = iovec {
                iov_base: self.data.start().cast(),
                iov_len: self.data.len(),
            };

            let msg = msghdr {
                msg_name: null_mut(),
                msg_namelen: 0,
                msg_iov: &mut iovec,
                msg_iovlen: 1,
                msg_control: self.ctrl.start().cast(),
                msg_controllen: self.ctrl.len(),
                msg_flags: self.flags,
            };

            trace!(
                socket,
                msg = ?msg_debug(&msg),
                flags,
                "sendmsg(socket, msg, flags)"
            );
            let res = libc::sendmsg(socket, &msg, flags);
            self.handle_res(socket, &msg, flags, res)
        }
    }

    fn handle_res(
        &mut self,
        socket: RawFd,
        msg: &msghdr,
        flags: c_int,
        res: ssize_t,
    ) -> Result<Option<Self>, c_int> {
        unsafe {
            match res {
                0 => {
                    trace!("fd closed");
                    Ok(None)
                }
                ret @ 1.. => {
                    let data = self
                        .data
                        .split_at(ret as usize)
                        .expect("data buf too short");

                    let ctrl = self
                        .ctrl
                        .split_at(msg.msg_controllen)
                        .expect("ctrl buf too short");

                    trace!(
                        socket,
                        msg = ?msg_debug(msg),
                        flags,
                        "msg(socket, msg, flags)"
                    );

                    Ok(Some(Self {
                        data,
                        ctrl,
                        flags: msg.msg_flags,
                    }))
                }
                -1 => {
                    let code = *__errno_location();
                    trace!(code, "err");
                    Err(code)
                }
                ..-1 => unreachable!(),
            }
        }
    }

    pub fn as_tuple(&self) -> (&[u8], &[u8], c_int) {
        unsafe { (&*self.data, &*self.ctrl, self.flags) }
    }
}

#[allow(nonstandard_style)]
struct msg_debug<'a>(&'a msghdr);

impl Debug for msg_debug<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        unsafe {
            f.debug_struct("msg")
                .field(
                    "name",
                    &if !self.0.msg_name.is_null() {
                        slice::from_raw_parts(
                            self.0.msg_name.cast::<u8>(),
                            self.0.msg_namelen as usize,
                        )
                        .as_bstr()
                    } else {
                        [].as_bstr()
                    },
                )
                .field(
                    "iov",
                    &if !self.0.msg_iov.is_null() {
                        slice::from_raw_parts(self.0.msg_iov, self.0.msg_iovlen)
                    } else {
                        &[]
                    },
                )
                .field(
                    "control",
                    &if !self.0.msg_control.is_null() {
                        slice::from_raw_parts(
                            self.0.msg_control.cast::<u8>(),
                            self.0.msg_controllen,
                        )
                    } else {
                        &[]
                    },
                )
                .field("flags", &self.0.msg_flags)
                .finish()
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::msg_io::{Msg, cmsg_cursor::CmsgCursor};
    use libc::{AF_UNIX, CMSG_SPACE, SCM_RIGHTS, SOCK_STREAM, SOL_SOCKET, cmsghdr, socketpair};
    use std::{
        io::{stdin, stdout},
        os::fd::{AsRawFd, RawFd},
    };
    use tracing::Level;

    #[test]
    #[allow(unused)]
    fn test_sockpair() {
        unsafe {
            tracing_subscriber::fmt()
                .with_max_level(Level::TRACE)
                .pretty()
                .init();

            let mut sv: [RawFd; 2] = [0, 0];
            let ret = socketpair(AF_UNIX, SOCK_STREAM, 0, &mut sv as *mut _);
            assert_eq!(ret, 0);

            {
                let mut data_buf: [u8; _] = [0, 1, 2, 3];
                let mut ctrl_buf: [u8; _] = [0; raw_fd_space(8)];

                let mut cursor = CmsgCursor::from_ctrl_buf(&mut ctrl_buf);
                cursor
                    .write_cursor::<RawFd>(SOL_SOCKET, SCM_RIGHTS)
                    .unwrap()
                    .write_slice(&[stdin().as_raw_fd(), stdout().as_raw_fd()])
                    .commit()
                    .unwrap();

                let mut msg = Msg {
                    data: &mut data_buf,
                    ctrl: cursor.as_slice(),
                    flags: 0,
                };

                let ctrl_bytes = [
                    24, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0,
                ];
                assert_eq!(
                    msg.as_tuple(),
                    ([0, 1, 2, 3].as_slice(), ctrl_bytes.as_slice(), 0)
                );
                assert_eq!(
                    msg.send(sv[0], 0).unwrap().unwrap().as_tuple(),
                    ([0, 1, 2, 3].as_slice(), ctrl_bytes.as_slice(), 0)
                );
                assert_eq!(msg.as_tuple(), ([].as_slice(), [].as_slice(), 0));

                let mut data_buf = [0; 8];
                let mut ctrl_buf = [0; raw_fd_space(8)];

                let mut msg = Msg {
                    data: &mut data_buf,
                    ctrl: &mut ctrl_buf,
                    flags: 0,
                };
                let recv = msg.recv(sv[1], 0).unwrap().unwrap();
                assert_eq!(
                    recv.as_tuple(),
                    (
                        [0, 1, 2, 3].as_slice(),
                        [
                            24, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 5, 0, 0, 0, 6, 0, 0,
                            0,
                        ]
                        .as_slice(),
                        0
                    )
                );
                assert_eq!(
                    msg.as_tuple(),
                    (
                        [0, 0, 0, 0].as_slice(),
                        [
                            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
                        ]
                        .as_slice(),
                        0
                    ),
                );

                let mut cursor = CmsgCursor::from_ctrl_buf(recv.ctrl);
                let (hdr, data) = cursor.read_cmsg().unwrap();
                assert_eq!(
                    hdr,
                    cmsghdr {
                        cmsg_len: 4 * 4 + 2 * 4,
                        cmsg_type: SOL_SOCKET,
                        cmsg_level: SCM_RIGHTS,
                    }
                );
                assert_eq!(*data.read_as::<RawFd>(), [5, 6]);
            }
        }
    }

    const fn raw_fd_space(u: u32) -> usize {
        unsafe { CMSG_SPACE(size_of::<RawFd>() as u32 * u) as usize }
    }
}
