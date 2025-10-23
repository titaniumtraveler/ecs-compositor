use crate::{
    connection::{DriveIo, Object},
    drive_io::Io,
    handle::{ConnectionHandle, InterfaceDir},
};
use ecs_compositor_core::{Interface, Message, Opcode, Value, message_header};
use std::{
    fmt::{self, Debug, Display},
    future::Future,
    io,
    marker::PhantomData,
    os::fd::{AsRawFd, RawFd},
    pin::Pin,
    sync::MutexGuard,
    task::{Context, Poll, ready},
};
use tracing::{debug, instrument, trace};

impl<Conn, I> Object<Conn, I>
where
    Conn: ConnectionHandle<Dir: InterfaceDir<I>>,
    I: Interface,
{
    pub fn recv(&self) -> Recv<'_, Conn, I, impl DriveIo> {
        Recv {
            obj: self,
            drive_io: self.conn().drive_io(),
        }
    }
}

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct Recv<'a, Conn, I, Fut>
where
    Conn: ConnectionHandle<Dir: InterfaceDir<I>>,
    I: Interface,
    Fut: DriveIo,
{
    obj: &'a Object<Conn, I>,
    drive_io: Fut,
}

impl<'a, Conn, I, Fut> Recv<'a, Conn, I, Fut>
where
    Conn: ConnectionHandle<Dir: InterfaceDir<I>>,
    I: Interface,
    Fut: DriveIo,
{
    fn drive_io(
        self: &mut Pin<&mut Self>,
        io: &mut Io,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
        match unsafe { self.as_mut().map_unchecked_mut(|s| &mut s.drive_io) }.poll_with_io(io, cx) {
            Poll::Ready(ready) => Poll::Ready(ready),
            Poll::Pending => Poll::Pending,
        }
    }

    fn fd(&self) -> RawFd {
        self.obj.conn().fd.as_raw_fd()
    }
}

impl<'a, Conn, I, Fut> Future for Recv<'a, Conn, I, Fut>
where
    Conn: ConnectionHandle<Dir: InterfaceDir<I>>,
    I: Interface,
    Fut: DriveIo,
    <Conn::Dir as InterfaceDir<I>>::Recv: Display,
{
    type Output = io::Result<MsgBuf<'a, Conn::Dir, I>>;
    #[instrument(name = "poll_recv", level = "trace", fields(fd = self.fd(), id = self.obj.id.id, interface = I::NAME), skip_all)]
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        unsafe {
            let obj = self.obj;
            let conn = self.obj.conn();

            let mut io = match conn.try_lock_io_buf() {
                Some(io) => io,
                None => {
                    trace!(return = ?Poll::<()>::Pending, "waiting on io lock");

                    obj.register_recv(cx);
                    return Poll::Pending;
                }
            };

            let mut count = 0;
            let (hdr, (_, buf)) = loop {
                trace!(count, "loop");
                count += 1;

                match io.rx_hdr {
                    None => {
                        let Some((_, buf)) = io.rx_msg_buf(message_header::COMBINED_LEN) else {
                            trace!("drive_io for header");
                            ready!(self.drive_io(&mut io, cx))?;
                            continue;
                        };

                        io.rx_hdr = Some(
                            message_header::read(
                                &mut buf.da.cast_const(),
                                &mut buf.fd.cast_const(),
                            )
                            .ok()
                            .expect("failed to read header"),
                        );
                        trace!(hdr = ?io.rx_hdr, "parsed header");
                        continue;
                    }
                    Some(hdr) => {
                        if obj.id.id() == hdr.object_id.id() {
                            let size = (
                                hdr.content_len(),
                                <Conn::Dir as InterfaceDir<I>>::Recv::from_u16(hdr.opcode)
                                    .map_err(|opcode| {
                                        format!(
                                            "invalid opcode {opcode} for ({name}@{version}) with id {id}",
                                            name = I::NAME,
                                            version = I::VERSION,
                                            id = hdr.object_id.id(),
                                        )
                                    })
                                    .unwrap()
                                    .fd_count(),
                            );
                            match io.rx_msg_buf(size) {
                                Some(data) => {
                                    io.rx_hdr = None;

                                    break (hdr, data);
                                }
                                None => {
                                    trace!("drive_io for ourself");
                                    ready!(self.drive_io(&mut io, cx))?;
                                    continue;
                                }
                            }
                        } else if let mut registry = obj.registry()
                            && let Some(entry) = { registry.receiver_map.get(&hdr.object_id) }
                        {
                            let size = (
                                hdr.content_len(),
                                (entry.fd_count)(hdr.opcode)
                                    .ok_or_else(|| {
                                        format!(
                                            "invalid opcode {opcode} for {id}",
                                            opcode = hdr.opcode,
                                            id = hdr.object_id.id(),
                                        )
                                    })
                                    .unwrap(),
                            );
                            match io.rx_msg_buf(size) {
                                Some((cursor, _)) => {
                                    trace!(
                                        from = %obj.id(),
                                        to = %hdr.object_id,
                                        "dispatching to object"
                                    );

                                    io.rx.restore_cursor(cursor);
                                    drop(io);

                                    entry.waker.wake_by_ref();
                                    registry.register_recv(obj.id, cx);

                                    return Poll::Pending;
                                }
                                None => {
                                    trace!(id = hdr.object_id.id().get(), "drive_io for other");
                                    ready!(self.drive_io(&mut io, cx))?;
                                    continue;
                                }
                            }
                        } else {
                            debug!(
                                return = ?Poll::<()>::Pending,
                                "`{obj}` received message addressed to unknown ID `{id}`, this *could* indicate a deadlock",
                                obj = obj,
                                id = hdr.object_id.id(),
                            );

                            obj.register_recv(cx);
                            return Poll::Pending;
                        }
                    }
                }
            };

            obj.register_recv(cx);
            obj.wake_recver(cx);

            trace!(id = %obj.id(), opcode = hdr.opcode, kind = %MsgKind::<Conn, I>::new(hdr.opcode), hdr = ?hdr, "recv");
            Poll::Ready(Ok(MsgBuf {
                _io: io,
                hdr,
                da: buf.da,
                fd: buf.fd,
                dir: PhantomData,
            }))
        }
    }
}

struct MsgKind<Conn, I>(u16, PhantomData<(Conn, I)>)
where
    Conn: ConnectionHandle<Dir: InterfaceDir<I>>,
    I: Interface;

impl<Conn, I> MsgKind<Conn, I>
where
    Conn: ConnectionHandle<Dir: InterfaceDir<I>>,
    I: Interface,
{
    fn new(opcode: u16) -> Self {
        Self(opcode, PhantomData)
    }
}

impl<Conn, I> Display for MsgKind<Conn, I>
where
    Conn: ConnectionHandle<Dir: InterfaceDir<I>>,
    I: Interface,
    <Conn::Dir as InterfaceDir<I>>::Recv: Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let iface = I::NAME;
        match <Conn::Dir as InterfaceDir<I>>::Recv::from_u16(self.0) {
            Ok(msg) => write!(f, "{iface}.{msg}#{opcode}", opcode = self.0,),
            Err(u16) => write!(f, "{iface}.<unknown>#{u16}"),
        }
    }
}

pub struct MsgBuf<'a, Dir: InterfaceDir<I>, I: Interface> {
    _io: MutexGuard<'a, Io>,
    hdr: message_header,
    da: *const [u8],
    fd: *const [RawFd],
    dir: PhantomData<(Dir, I)>,
}

impl<'a, Dir: InterfaceDir<I>, I: Interface> Debug for MsgBuf<'a, Dir, I> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Debug::fmt(&self.hdr, f)
    }
}

impl<'a, Dir, I> MsgBuf<'a, Dir, I>
where
    Dir: InterfaceDir<I>,
    I: Interface,
{
    pub fn hdr(&self) -> message_header {
        self.hdr
    }

    pub fn decode_opcode(&self) -> Dir::Recv {
        Dir::Recv::from_u16(self.hdr.opcode)
            .map_err(|opcode| {
                format!(
                    "invalid opcode {opcode} for ({name}@{version}) with id {id}",
                    name = I::NAME,
                    version = I::VERSION,
                    id = self.hdr.object_id.id(),
                )
            })
            .unwrap()
    }

    pub fn decode_msg<'data, M: Message<'data>>(
        &'data self,
    ) -> ecs_compositor_core::primitives::Result<M> {
        let (mut da, mut fd) = (self.da, self.fd);

        unsafe { M::read(&mut da, &mut fd) }
    }

    pub fn ignore_message(self) {}
}
