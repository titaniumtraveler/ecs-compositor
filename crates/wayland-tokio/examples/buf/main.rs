use crate::protocols::wayland::{wl_display, wl_registry};
use ecs_compositor_core::{Message, RawSliceExt, Value, message_header, new_id};
use ecs_compositor_tokio::{
    buf::{
        AsyncRecv, IoCount,
        recv::{self, Info, RecvBuf, RecvRef, io::RecvState},
    },
    msg_io::cmsg_cursor::{self, CmsgCursor},
};
use ecs_helpers::tracing::setup_tracing;
use futures::future;
use libc::{CMSG_SPACE, MSG_DONTWAIT, SCM_RIGHTS, SOL_SOCKET, cmsghdr, msghdr};
use std::{
    cmp, env,
    fmt::Debug,
    io::{self, Write},
    marker::{PhantomData, PhantomPinned},
    mem,
    num::{NonZero, NonZeroU32},
    os::{
        fd::{AsRawFd, RawFd},
        unix::net::{SocketAddr, UnixStream},
    },
    path::PathBuf,
    pin::pin,
    ptr::null_mut,
    sync::atomic::Ordering::Relaxed,
    task::{Context, Poll, ready},
};
use tokio::io::unix::AsyncFd;
use tracing::{debug, info, instrument, trace, warn};

mod protocols;

#[tokio::main]
async fn main() -> io::Result<()> {
    inner().await
}

async fn inner() -> io::Result<()> {
    setup_tracing();

    let mut buf = RecvBuf::new();
    let buf = unsafe { RecvRef::new(&mut buf) };
    let mut state = RecvState::new();

    let mut sock = pin!(Sock::new());
    let mut callback = Callback::new();

    send_get_registry(sock.fd.get_ref())?;

    info!(
        buf.free = ?Info(buf.atomic_state().free.load(Relaxed)),
        buf.wait = ?Info(buf.atomic_state().wait.load(Relaxed)),
        buf.recv = ?state.recv,
        "initial state",
    );

    loop {
        let (handle, io_err, parse_err) = future::poll_fn(|cx| state.recv(sock.as_mut(), buf, &mut callback, cx)).await;
        let Some(mut handle) = handle else {
            info!(
                err = ?(io_err, parse_err),

                handle = ?handle,

                buf.free = ?Info(buf.atomic_state().free.load(Relaxed)),
                buf.wait = ?Info(buf.atomic_state().wait.load(Relaxed)),
                buf.recv = ?state.recv,

                state.socket_closed = ?state.socket_closed,

                state.recv_data_hold = ?state.recv_data_hold,
                state.recv_ctrl_hold = ?state.recv_ctrl_hold,

                state.free_data_hold = ?state.free_data_hold,
                state.free_ctrl_hold = ?state.free_ctrl_hold,

                sock = ?UnixSock::new(&sock.fd),

                ?callback,
            );

            break;
        };
        info!(
            err = ?(io_err, parse_err),

            handle.slot.start = ?handle.slot().start(),
            handle.slot.end   = ?handle.slot().end(),
            handle.data = ?handle.data(),
            handle.ctrl = ?handle.ctrl(),
            handle.free = ?handle.free(),

            buf.free = ?Info(buf.atomic_state().free.load(Relaxed)),
            buf.wait = ?Info(buf.atomic_state().wait.load(Relaxed)),
            buf.recv = ?state.recv,

            state.socket_closed = ?state.socket_closed,

            state.recv_data_hold = ?state.recv_data_hold,
            state.recv_ctrl_hold = ?state.recv_ctrl_hold,

            state.free_data_hold = ?state.free_data_hold,
            state.free_ctrl_hold = ?state.free_ctrl_hold,

            sock = ?UnixSock::new(&sock.fd),

            ?callback,
        );

        {
            use {wl_display::event as wl_display, wl_registry::event as wl_registry};

            let mut cursor = handle.cursor();
            while let Some(mut buf) = cursor.read_msg(|hdr| {
                Ok(match (hdr.object_id.id().get(), hdr.opcode) {
                    (1, wl_display::delete_id::OP) => wl_display::delete_id::FDS,
                    (1, wl_display::error::OP) => wl_display::error::FDS,
                    (2, wl_registry::global::OP) => wl_registry::global::FDS,
                    (2, wl_registry::global_remove::OP) => wl_registry::global_remove::FDS,
                    _ => unreachable!(),
                })
            })? {
                let hdr = buf.header();
                match (hdr.object_id.id().get(), hdr.opcode) {
                    (1, wl_display::error::OP) => {
                        let msg: wl_display::error = buf.msg()?;
                        println!("{msg}");
                    }
                    (1, wl_display::delete_id::OP) => {
                        let msg: wl_display::delete_id = buf.msg()?;
                        println!("{msg}");
                    }
                    (2, wl_registry::global::OP) => {
                        let msg: wl_registry::global = buf.msg()?;
                        println!("{msg}");
                    }
                    (2, wl_registry::global_remove::OP) => {
                        let msg: wl_registry::global_remove = buf.msg()?;
                        println!("{msg}");
                    }
                    _ => unreachable!(),
                };
            }
            println!()
        }
    }

    Ok(())
}

fn send_get_registry(mut sock: &UnixStream) -> io::Result<()> {
    use protocols::wayland::wl_display::request as wl_display;
    let display: new_id = new_id { id: NonZero::new(1).unwrap(), _marker: PhantomData };
    let registry = new_id { id: NonZero::new(2).unwrap(), _marker: PhantomData };

    let mut data = [0u8; 0x1000];

    let msg = wl_display::get_registry { registry };
    let datalen = message_header::DATA_LEN + msg.len() as u16;
    let hdr = message_header { object_id: display.to_object(), datalen, opcode: wl_display::get_registry::OP };

    unsafe {
        let mut data: *mut [u8] = data.as_mut_slice();
        let mut ctrl: *mut [RawFd] = &mut [];
        hdr.write(&mut data, &mut ctrl)?;
        msg.write(&mut data, &mut ctrl)?;
    }

    info!(data = ?data[..datalen as usize]);
    sock.write_all(&data[..datalen as usize])?;
    let invalid_wayland_val: &[u8] = &[
        0, 0, 0, 0, //
        0, 0, 0, 0,
    ];
    sock.write_all(invalid_wayland_val)?;
    Ok(())
}

struct Sock {
    fd: AsyncFd<UnixStream>,
    cmsg_buf: [u8; unsafe { CMSG_SPACE(4 * 252) as usize }],

    cmsg_cursor: Option<CmsgCursor>,
    ctrl_cursor: Option<cmsg_cursor::ReadData>,

    _pinned: PhantomPinned,
}

impl Debug for Sock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self { fd, cmsg_buf: _, cmsg_cursor, ctrl_cursor, _pinned: _ } = self;
        f.debug_struct("Sock")
            .field("fd", &fd)
            // .field("cmsg_buf", &"<omitted>")
            .field("cmsg_cursor", &cmsg_cursor)
            .field("ctrl_cursor", &ctrl_cursor)
            // .field("_pinned", &"<omitted>")
            .finish()
    }
}

impl Sock {
    fn new() -> Self {
        let sock = UnixStream::connect(PathBuf::from_iter([
            env::var_os("XDG_RUNTIME_DIR").unwrap(),
            env::var_os("WAYLAND_DISPLAY").unwrap(),
        ]))
        .unwrap();
        let fd = AsyncFd::new(sock).unwrap();

        Self { fd, cmsg_buf: [0; _], cmsg_cursor: None, ctrl_cursor: None, _pinned: PhantomPinned }
    }
}

impl AsyncRecv for Sock {
    fn poll_recv(
        self: std::pin::Pin<&mut Self>,
        data_buf: &mut &mut [libc::iovec],
        ctrl_buf: &mut &mut [libc::iovec],
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<IoCount>> {
        let Self { fd, cmsg_buf, cmsg_cursor, ctrl_cursor, .. } = unsafe { self.get_unchecked_mut() };
        // info!(fd = ?fd.get_ref(), ?cmsg_cursor, ?ctrl_cursor, "Sock::poll_recv()");
        'recv: loop {
            let mut guard = ready!({
                let poll = fd.poll_read_ready(cx);
                debug!(?fd, ?poll);
                poll
            })?;
            let (data_count, cursor) = match &mut *cmsg_cursor {
                cursor @ None => {
                    match guard.try_io(|fd| {
                        let mut msg = msghdr {
                            msg_name: null_mut(),
                            msg_namelen: 0,
                            msg_iov: data_buf.as_mut_ptr(),
                            msg_iovlen: data_buf.len(),
                            msg_control: cmsg_buf.as_mut_ptr().cast(),
                            msg_controllen: cmsg_buf.len(),
                            msg_flags: 0,
                        };
                        let res = unsafe {
                            match libc::recvmsg(fd.as_raw_fd(), &mut msg, MSG_DONTWAIT) {
                                0 => {
                                    trace!("fd closed");
                                    Ok(None)
                                }
                                ret @ 1.. => Ok(Some((ret as usize, msg))),
                                -1 => {
                                    let code = *libc::__errno_location();
                                    Err(io::Error::from_raw_os_error(code))
                                }
                                ..-1 => unreachable!(),
                            }
                        };
                        debug!(?res);

                        res
                    }) {
                        Ok(Ok(None)) => return Poll::Ready(Ok(IoCount { data: 0, ctrl: 0 })),
                        Ok(Ok(Some((count, msg)))) => (count, cursor.insert(CmsgCursor::new(msg))),
                        Ok(Err(err)) => return Poll::Ready(Err(err)),
                        Err(_) => return Poll::Pending,
                    }
                }
                Some(cursor) => (0, cursor),
            };

            let empty_read_data = &mut cmsg_cursor::ReadData { data: &mut [] };
            let duplicate_scm_rights = false;
            let ctrl = match &mut *ctrl_cursor {
                Some(data) => data,
                ctrl @ None => loop {
                    match cursor.read_cmsg() {
                        Some((cmsghdr { cmsg_type: SOL_SOCKET, cmsg_level: SCM_RIGHTS, .. }, ctrl_data))
                            if !duplicate_scm_rights =>
                        {
                            break ctrl.insert(ctrl_data);
                        }
                        Some((cmsghdr { cmsg_type: SOL_SOCKET, cmsg_level: SCM_RIGHTS, .. }, ctrl_data)) => {
                            warn!("duplicate SCM_RIGHTS control message");
                            break ctrl.insert(ctrl_data);
                        }
                        Some((cmsghdr { cmsg_type, cmsg_level, cmsg_len }, _ctrl_data)) => {
                            trace!(
                                fd = guard.get_inner().as_raw_fd(),
                                cmsg_type, cmsg_level, cmsg_len, "unknown cmsg type, discarding"
                            );
                        }
                        None if 0 < data_count => {
                            *cmsg_cursor = None;
                            *ctrl = None;
                            break empty_read_data;
                        }
                        None => {
                            *cmsg_cursor = None;
                            *ctrl = None;
                            continue 'recv;
                        }
                    }
                },
            };

            {
                let mut remove = 0;
                let mut left = data_count;
                for buf in data_buf.iter_mut() {
                    if let Some(remainder) = left.checked_sub(buf.iov_len) {
                        left = remainder;
                        remove += 1;
                    } else {
                        buf.iov_base = unsafe { buf.iov_base.cast::<u8>().add(left).cast() };
                        buf.iov_len -= left;

                        *data_buf = &mut mem::take(data_buf)[remove..];

                        break;
                    }
                }
            }

            let mut ctrl_count = 0;
            'ctrl: {
                if ctrl.data.is_empty() {
                    break 'ctrl;
                }
                let mut remove = 0;
                for buf in ctrl_buf.iter_mut() {
                    unsafe {
                        let len = cmp::min(ctrl.data.len(), buf.iov_len);
                        assert_eq!(len & 0b11, 0);
                        let src = ctrl.data.split_at(len).expect("length should never be too small!");
                        buf.iov_base.copy_from(src.cast(), len);
                        ctrl_count += len / 4;

                        if ctrl.data.is_empty() {
                            buf.iov_base = buf.iov_base.cast::<u8>().add(len).cast();
                            buf.iov_len -= len;

                            *ctrl_cursor = None;
                            break;
                        }

                        remove += 1;
                    }
                }

                *ctrl_buf = &mut mem::take(ctrl_buf)[remove..];
            }

            debug!(data_count, ctrl_count, "return");
            break Poll::Ready(Ok(IoCount { data: data_count, ctrl: ctrl_count }));
        }
    }
}

#[derive(Debug)]
struct Callback {
    last_object: Option<NonZeroU32>,
}

impl Callback {
    fn new() -> Self {
        Self { last_object: None }
    }
}

impl recv::io::Callback for Callback {
    fn fd_count(&mut self, _hdr: message_header) -> usize {
        0
    }

    #[instrument]
    fn should_include(&mut self, hdr: message_header) -> bool {
        trace!("should_include");
        match self.last_object {
            Some(id) => id == hdr.object_id.id(),
            None => {
                self.last_object = Some(hdr.object_id.id());
                true
            }
        }
    }

    fn slot_count(&mut self, count: usize) -> usize {
        count
    }

    fn finish(&mut self) {
        self.last_object = None;
    }
}

#[allow(dead_code)]
#[derive(Debug)]
struct UnixSock {
    fd: RawFd,
    local: SocketAddr,
    peer: SocketAddr,
}

impl UnixSock {
    pub fn new(sock: &AsyncFd<UnixStream>) -> Self {
        let sock = sock.get_ref();
        Self { fd: sock.as_raw_fd(), local: sock.local_addr().unwrap(), peer: sock.peer_addr().unwrap() }
    }
}
