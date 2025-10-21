use crate::connection::{Connection, InterfaceDir, Io, Object, ready_fut::DriveIo};
use ecs_compositor_core::{Interface, Message, Opcode, Value, message_header};
use std::fmt::{self, Debug};
use std::os::fd::AsRawFd;
use std::{
    future::Future,
    io,
    marker::PhantomData,
    os::fd::RawFd,
    pin::Pin,
    sync::MutexGuard,
    task::{Context, Poll, ready},
};
use tracing::{debug, instrument, trace};

impl<Conn, I, Dir> Object<Conn, I, Dir>
where
    Conn: AsRef<Connection<Dir>>,
    I: Interface,
    Dir: InterfaceDir<I>,
{
    pub async fn recv<'a>(&'a self) -> io::Result<MsgBuf<'a, Dir, I>> {
        Recv {
            obj: self,
            drive_io: self.drive_io(),
        }
        .await
    }
}

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct Recv<'a, Conn, I, Dir, Fut>
where
    Conn: AsRef<Connection<Dir>>,
    I: Interface,
    Dir: InterfaceDir<I>,
    Fut: DriveIo,
{
    obj: &'a Object<Conn, I, Dir>,
    drive_io: Fut,
}

impl<'a, Conn, I, Dir, Fut> Recv<'a, Conn, I, Dir, Fut>
where
    Conn: AsRef<Connection<Dir>>,
    I: Interface,
    Dir: InterfaceDir<I>,
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
        self.obj.conn.as_ref().fd.as_raw_fd()
    }
}

impl<'a, Conn, I, Dir, Fut> Future for Recv<'a, Conn, I, Dir, Fut>
where
    Conn: AsRef<Connection<Dir>>,
    I: Interface,
    Dir: InterfaceDir<I>,
    Fut: DriveIo,
{
    type Output = io::Result<MsgBuf<'a, Dir, I>>;
    #[instrument(name = "poll_recv", level = "trace", fields(fd = self.fd(), id = self.obj.id.id, interface = I::NAME), skip_all)]
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        unsafe {
            let obj = self.obj;
            let conn = self.obj.conn.as_ref();

            let mut io = match conn.try_lock_io_buf() {
                Some(io) => io,
                None => {
                    trace!(return = ?Poll::<()>::Pending, "waiting on io lock");

                    obj.register_recv(cx);
                    return Poll::Pending;
                }
            };

            let (hdr, (_, buf)) = loop {
                trace!("loop");
                match io.rx_hdr {
                    None => {
                        let Some((_, buf)) = io.rx_msg_buf(message_header::COMBINED_LEN) else {
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
                                Dir::Recv::from_u16(hdr.opcode)
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
                                    trace!(return = ?Poll::<()>::Pending, id = hdr.object_id.id(), "dispatching to object");

                                    io.rx.restore_cursor(cursor);
                                    drop(io);

                                    entry.waker.wake_by_ref();
                                    registry.register_recv(obj.id, cx);

                                    return Poll::Pending;
                                }
                                None => {
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

            obj.wake_recver(cx);

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
}
