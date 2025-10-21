use crate::msg_io::{Msg, cmsg_cursor::CmsgCursor};
use bitflags::bitflags;
use ecs_compositor_core::{Message, RawSliceExt, Value, message_header, object};
use libc::{CMSG_SPACE, EWOULDBLOCK, MSG_DONTWAIT, SCM_RIGHTS, SOL_SOCKET, cmsghdr};
use std::{
    alloc::{self, Layout},
    cmp,
    fmt::{self, Debug, Display, Formatter},
    io,
    os::{
        fd::{AsRawFd, RawFd},
        unix::net::UnixStream,
    },
    ptr::{null_mut, slice_from_raw_parts_mut},
};
use tokio::io::{Ready, unix::AsyncFdReadyGuard};
use tracing::{instrument, trace, warn};

#[derive(Debug)]
pub(crate) struct Io {
    pub(crate) tx: BufDir,
    pub(crate) rx: BufDir,

    pub(crate) interest: Interest,
    pub(crate) rx_hdr: Option<message_header>,

    cmsg_buf: [u8; unsafe { CMSG_SPACE(4 * MAX_FDS) as usize }],
}

bitflags! {
    #[derive(Debug, PartialEq, Eq, Clone, Copy)]
    pub struct Interest: u8 {
        const RECV        = 1 << 1;
        const SEND        = 1 << 2;
        const RECV_CLOSED = 1 << 3;
        const SEND_CLOSED = 1 << 4;
    }
}

impl Display for Interest {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let mut iter = self.iter_names();

        let (str, _) = iter.next().unwrap_or(("EMPTY", Self::empty()));
        f.write_str(str)?;

        for (str, _) in iter {
            f.write_str(" | ")?;
            f.write_str(str)?;
        }

        Ok(())
    }
}

fn io_ready(guard: &AsyncFdReadyGuard<UnixStream>) -> Interest {
    let ready = guard.ready();
    let mut out = Interest::empty();
    if ready.is_readable() {
        out.insert(Interest::RECV);
    }
    if ready.is_read_closed() {
        out.insert(Interest::RECV_CLOSED);
    }
    if ready.is_writable() {
        out.insert(Interest::SEND);
    }
    if ready.is_write_closed() {
        out.insert(Interest::SEND_CLOSED);
    }

    out
}

impl Io {
    pub fn new() -> Self {
        Io {
            tx: BufDir::new(),
            rx: BufDir::new(),

            rx_hdr: None,
            cmsg_buf: [0; _],

            interest: Interest::RECV,
        }
    }

    pub fn query_interest(&self) -> Option<tokio::io::Interest> {
        match self.interest {
            interest if interest.contains(Interest::RECV | Interest::SEND) => {
                Some(tokio::io::Interest::READABLE | tokio::io::Interest::WRITABLE)
            }
            interest if interest.contains(Interest::RECV) => Some(tokio::io::Interest::READABLE),
            interest if interest.contains(Interest::SEND) => Some(tokio::io::Interest::WRITABLE),
            _ => None,
        }
    }

    #[instrument(name = "drive_io", level = "trace", fields(interest = %self.interest, ready = %io_ready(guard)), ret, skip_all)]
    pub fn drive_io(&mut self, guard: &mut AsyncFdReadyGuard<UnixStream>) -> io::Result<()> {
        let ready = guard.ready();

        let mut reading = self.interest.contains(Interest::RECV) && ready.is_readable();
        let mut writing = self.interest.contains(Interest::SEND) && ready.is_writable();

        let mut count = 0;
        loop {
            if !reading && !writing {
                break;
            }

            if reading {
                reading = self.recv(guard)?;
            }

            if writing {
                writing = self.send(guard)?;
            }

            count += 1;
            trace!(reading, writing, count)
        }

        Ok(())
    }

    #[instrument(name = "client rx", level = "trace", fields(fd = guard.get_inner().as_raw_fd()), ret, skip_all)]
    fn recv(&mut self, guard: &mut AsyncFdReadyGuard<UnixStream>) -> io::Result<bool> {
        unsafe {
            let da = &mut self.rx.da;
            let fd = &mut self.rx.fd;
            let mut ctrl = &mut self.cmsg_buf as *mut [u8];

            if self.interest.contains(Interest::RECV_CLOSED) {
                self.interest.remove(Interest::RECV);
                return Ok(false);
            }

            let data = 'data: {
                // reset data buf and return whole buf
                if da.data.is_empty() {
                    da.data = slice_from_raw_parts_mut(da.buf.start(), 0);

                    let mut data = da.buf;
                    data.set_len(WAYLAND_MAX_MESSAGE_LEN * 3);

                    break 'data data;
                }

                const HDR_LEN: usize = 8;
                let mut unused = da.unused_end();
                if unused.len() < WAYLAND_MAX_MESSAGE_LEN * 2 {
                    match self.rx_hdr {
                        None if HDR_LEN <= da.data.len() => {
                            self.interest.remove(Interest::RECV);
                            return Ok(false);
                        }
                        None => {
                            let len = da.data.len() - HDR_LEN;
                            unused.set_len(len);
                            unused
                        }

                        Some(hdr) if hdr.content_len() as usize <= da.data.len() => {
                            self.interest.remove(Interest::RECV);
                            return Ok(false);
                        }
                        Some(hdr) => {
                            let len = hdr.content_len() as usize - da.data.len();
                            unused.set_len(len);
                            unused
                        }
                    }
                } else {
                    unused.set_len(WAYLAND_MAX_MESSAGE_LEN * 3);
                    unused
                }
            };

            let mut ctrl_dst = 'fd: {
                if fd.data.is_empty() {
                    fd.data = slice_from_raw_parts_mut(fd.buf.start(), 0);

                    break 'fd fd.unused_end();
                }

                let unused = fd.unused_end();
                if unused.len() < 4 {
                    let data = fd.data;
                    fd.buf.start().copy_from(data.start(), data.len());
                    fd.data = slice_from_raw_parts_mut(fd.buf.start(), data.len());
                    fd.unused_end()
                } else {
                    unused
                }
            };
            ctrl.set_len(cmp::min(
                ctrl.len(),
                CMSG_SPACE((ctrl_dst.len() * size_of::<RawFd>()) as u32) as usize,
            ));

            let mut msg = Msg {
                data,
                ctrl,
                flags: 0,
            };

            match msg.recv(guard.get_inner().as_raw_fd(), MSG_DONTWAIT) {
                // fd closed on the other side
                Ok(None) => {
                    trace!(fd = ?guard.get_inner(), "closed");
                    self.interest.remove(Interest::RECV);
                    self.interest.insert(Interest::RECV_CLOSED);

                    Ok(false)
                }
                Ok(Some(msg)) => {
                    trace!(
                        fd = guard.get_inner().as_raw_fd(),
                        data_len = msg.data.len(),
                        ctrl_len = msg.ctrl.len(),
                        "received data"
                    );

                    da.data.set_len(da.data.len() + msg.data.len());

                    let mut cursor = CmsgCursor::from_ctrl_buf(msg.ctrl);

                    loop {
                        match cursor.read_cmsg() {
                            Some((
                                cmsghdr {
                                    cmsg_type: SOL_SOCKET,
                                    cmsg_level: SCM_RIGHTS,
                                    ..
                                },
                                ctrl_data,
                            )) if !ctrl_dst.is_null() => {
                                let fds = ctrl_data.read_as::<RawFd>();
                                assert!(fds.len() <= ctrl_dst.len());

                                ctrl_dst.start().copy_from(fds.start(), fds.len());
                                fd.data.set_len(fd.data.len() + fds.len());

                                ctrl_dst = slice_from_raw_parts_mut(null_mut(), 0);
                            }
                            Some((
                                cmsghdr {
                                    cmsg_type: SOL_SOCKET,
                                    cmsg_level: SCM_RIGHTS,
                                    ..
                                },
                                _ctrl_data,
                            )) => {
                                warn!("duplicate SCM_RIGHTS control message");
                            }

                            Some((
                                cmsghdr {
                                    cmsg_type,
                                    cmsg_level,
                                    cmsg_len,
                                },
                                _ctrl_data,
                            )) => {
                                trace!(
                                    fd = guard.get_inner().as_raw_fd(),
                                    cmsg_type,
                                    cmsg_level,
                                    cmsg_len,
                                    "unknown cmsg type, discarding"
                                );
                            }
                            None => {
                                break;
                            }
                        }
                    }

                    Ok(true)
                }
                Err(code) if code == EWOULDBLOCK => {
                    guard.clear_ready_matching(Ready::READABLE);

                    Ok(false)
                }
                Err(code) => Err(io::Error::from_raw_os_error(code)),
            }
        }
    }

    #[instrument(name = "client tx", level = "trace", fields(fd = guard.get_inner().as_raw_fd()), ret, skip_all)]
    fn send(&mut self, guard: &mut AsyncFdReadyGuard<UnixStream>) -> io::Result<bool> {
        unsafe {
            let da = &mut self.tx.da;
            let fd = &mut self.tx.fd;

            if da.data.is_empty() || self.interest.contains(Interest::SEND_CLOSED) {
                trace!("data empty");

                self.interest.remove(Interest::SEND);
                return Ok(false);
            }

            let data = da.data;
            let ctrl = 'ctrl: {
                if fd.data.is_empty() {
                    trace!("fd.data is empty");
                    break 'ctrl slice_from_raw_parts_mut(null_mut(), 0);
                }

                let mut ctrl = fd.data;
                ctrl.set_len(cmp::min(ctrl.len(), MAX_FDS as usize));

                let mut cursor = CmsgCursor::from_ctrl_buf(&mut self.cmsg_buf);
                cursor
                    .write_cursor(SOL_SOCKET, SCM_RIGHTS)
                    .expect("failed to create tx cmsg buffer")
                    .write_slice(&*ctrl)
                    .commit()
                    .unwrap();
                cursor.as_slice()
            };

            let mut msg = Msg {
                data,
                ctrl,
                flags: 0,
            };

            match msg.send(guard.get_inner().as_raw_fd(), MSG_DONTWAIT) {
                // fd closed on the other side
                Ok(None) => {
                    trace!("closed");

                    self.interest.remove(Interest::SEND);
                    self.interest.insert(Interest::SEND_CLOSED);

                    Ok(false)
                }
                Ok(Some(msg)) => {
                    trace!(
                        data_len = msg.data.len(),
                        ctrl_len = msg.ctrl.len(),
                        "sent data"
                    );

                    da.data.split_at(msg.data.len()).unwrap();
                    fd.data
                        .split_at(cmp::min(fd.data.len(), MAX_FDS as usize))
                        .unwrap();

                    if da.data.is_empty() {
                        self.interest.remove(Interest::SEND);
                        return Ok(false);
                    }

                    Ok(true)
                }
                Err(code) if code == EWOULDBLOCK => {
                    guard.clear_ready_matching(Ready::WRITABLE);

                    Ok(false)
                }
                Err(code) => Err(io::Error::from_raw_os_error(code)),
            }
        }
    }

    #[instrument(name = "tx buf write", level = "trace", ret, skip_all)]
    pub fn tx_msg_buf<'a, M>(
        &mut self,
        object_id: object<M::Interface>,
        msg: &M,
    ) -> Option<(IoBuf, IoBuf)>
    where
        M: Message<'a>,
    {
        unsafe {
            let tx = &mut self.tx;
            let cursor = tx.save_cursor();

            let data_len = message_header::DATA_LEN as usize + msg.len() as usize;
            let ctrl_len = message_header::CTRL_LEN + M::FDS;

            trace!(
                expected_data = data_len,
                expected_ctrl = ctrl_len,
                actual_data = tx.da.unused_end().len(),
                actual_ctrl = tx.fd.unused_end().len(),
                "send buf write"
            );

            if !self.interest.contains(Interest::SEND_CLOSED) {
                self.interest.insert(Interest::SEND);
            }

            match (
                tx.da.unused_end().split_at(data_len),
                tx.fd.unused_end().split_at(ctrl_len),
            ) {
                (Some(mut da), Some(mut fd)) => {
                    tx.da.data.set_len(tx.da.data.len() + data_len);
                    tx.fd.data.set_len(tx.fd.data.len() + ctrl_len);

                    message_header {
                        object_id: object_id.cast(),
                        datalen: da.len() as u16,
                        opcode: M::OP,
                    }
                    .write(&mut da, &mut fd)
                    .ok()
                    .expect("failed writing message_header");

                    Some((cursor, IoBuf { da, fd }))
                }
                _ => {
                    trace!("failure");
                    None
                }
            }
        }
    }

    #[instrument(name = "rx buf write read", level = "trace", fields(data_len = da, ctrl_len = fd), ret, skip_all)]
    pub fn rx_msg_buf(&mut self, (da, fd): (u16, usize)) -> Option<(IoBuf, IoBuf)> {
        unsafe {
            let rx = &mut self.rx;
            let cursor = rx.save_cursor();

            trace!(
                expected_data = da,
                expected_ctrl = fd,
                actual_data = self.rx.da.data.len(),
                actual_ctrl = self.rx.fd.data.len(),
                "recv buf read"
            );

            let data_len = da as usize;
            let ctrl_len = fd;

            match (
                self.rx.da.data.split_at(data_len),
                self.rx.fd.data.split_at(ctrl_len),
            ) {
                (Some(da), Some(fd)) => Some((cursor, IoBuf { da, fd })),
                _ => {
                    if !self.interest.contains(Interest::RECV_CLOSED) {
                        self.interest.insert(Interest::RECV)
                    }

                    self.rx.restore_cursor(cursor);
                    None
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct BufDir {
    da: RingBuf<u8>,
    fd: RingBuf<RawFd>,
}

impl BufDir {
    pub fn new() -> Self {
        unsafe {
            let da = RingBuf::new(Layout::from_size_align_unchecked(MAX_DATA, 1), MAX_DATA);
            let fd = RingBuf::new(Layout::new::<RawFd>(), 1024);

            Self { da, fd }
        }
    }

    pub fn is_empty(&self) -> bool {
        // linux doesn't allow for sending only `msg_control`, so when there is no data to send,
        // there is nothing to send
        self.da.data.is_empty()
    }
}

impl BufDir {
    pub fn save_cursor(&mut self) -> IoBuf {
        IoBuf {
            da: self.da.data,
            fd: self.fd.data,
        }
    }

    pub fn restore_cursor(&mut self, cursor: IoBuf) {
        self.da.data = cursor.da;
        self.fd.data = cursor.fd;
    }
}

#[derive(Debug)]
pub struct IoBuf {
    pub da: *mut [u8],
    pub fd: *mut [RawFd],
}

pub struct RingBuf<T> {
    buf: *mut [T],
    data: *mut [T],
}

unsafe impl<T: std::marker::Send> std::marker::Send for RingBuf<T> {}

impl<T> Debug for RingBuf<T>
where
    T: Debug,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        unsafe {
            f.debug_struct("Buf")
                .field(
                    "buf",
                    &format_args!(
                        "[{addr:?},{len}]",
                        addr = self.buf.start(),
                        len = self.buf.len()
                    ),
                )
                .field(
                    "data",
                    &format_args!(
                        "[{addr:?},{len}]",
                        addr = self.data.start(),
                        len = self.data.len()
                    ),
                )
                .finish()
        }
    }
}

impl<T> RingBuf<T> {
    /// Allocates `layout` and creates an
    ///
    /// # Safety
    ///
    /// - The layouts alignment must be sufficient for `T`
    ///   `align_of::<T>() <= layout.align()`
    /// - `<*mut T>.add(len)` has to point to the end of the buffer
    unsafe fn new(layout: Layout, len: usize) -> RingBuf<T> {
        unsafe {
            let alloc = slice_from_raw_parts_mut(alloc::alloc(layout).cast(), len);

            if alloc.is_null() {
                panic!("alloc failed {alloc:p}");
            }

            Self {
                buf: alloc,
                data: slice_from_raw_parts_mut(alloc.cast(), 0),
            }
        }
    }

    #[allow(unused)]
    fn unused_start(&self) -> *mut [T] {
        unsafe { <*mut [T]>::from_range(self.buf.start(), self.data.start()) }
    }

    fn unused_end(&self) -> *mut [T] {
        unsafe { <*mut [T]>::from_range(self.data.end(), self.buf.end()) }
    }
}

pub const WAYLAND_MAX_MESSAGE_LEN: usize = 1 << 16;
pub const MAX_DATA: usize = WAYLAND_MAX_MESSAGE_LEN * 4;
pub const MAX_FDS: u32 = 252;
