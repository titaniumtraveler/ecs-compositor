use crate::{
    connection::{Connection, DriveIo, Object},
    drive_io::{Interest, Io},
    handle::{ConnectionHandle, InterfaceDir},
};
use ecs_compositor_core::{Interface, Message};
use std::{
    fmt::Display,
    future::Future,
    io,
    os::fd::{AsRawFd, RawFd},
    pin::Pin,
    task::{Context, Poll, ready},
};
use tracing::{instrument, trace};

impl<Conn, I> Object<Conn, I>
where
    Conn: ConnectionHandle<Dir: InterfaceDir<I>>,
    I: Interface,
{
    #[instrument(level = "trace", skip(self, msg), fields(%msg))]
    pub fn send<'a, Msg>(&'a self, msg: &'a Msg) -> Send<'a, Conn, I, Msg, impl DriveIo>
    where
        Msg: Message<'a, Opcode = <Conn::Dir as InterfaceDir<I>>::Send, Interface = I> + Display,
    {
        Send {
            obj: self,
            msg,
            ready_fut: self.conn().drive_io(),
            did_send: false,
        }
    }
}

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct Send<'a, Conn, I, Msg, Fut>
where
    Conn: ConnectionHandle<Dir: InterfaceDir<I>>,
    I: Interface,
    Msg: Message<'a, Opcode = <Conn::Dir as InterfaceDir<I>>::Send, Interface = I>,
    Fut: DriveIo,
{
    obj: &'a Object<Conn, I>,
    msg: &'a Msg,
    ready_fut: Fut,
    did_send: bool,
}

impl<'a, Conn, I, Msg, Fut> Send<'a, Conn, I, Msg, Fut>
where
    Conn: ConnectionHandle<Dir: InterfaceDir<I>>,
    I: Interface,
    Msg: Message<'a, Opcode = <Conn::Dir as InterfaceDir<I>>::Send, Interface = I>,
    Fut: DriveIo,
{
    fn ready_fut<'pin>(self: &'pin mut Pin<&mut Self>) -> Pin<&'pin mut Fut> {
        unsafe { self.as_mut().map_unchecked_mut(|s| &mut s.ready_fut) }
    }

    fn drive_io(
        self: &mut Pin<&mut Self>,
        io: &mut Io,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
        self.ready_fut().poll_with_io(io, cx)
    }

    fn fd(&self) -> RawFd {
        self.obj.conn().fd.as_raw_fd()
    }
}

impl<'a, Conn, I, Msg, Fut> Future for Send<'a, Conn, I, Msg, Fut>
where
    Conn: ConnectionHandle<Dir: InterfaceDir<I>>,
    I: Interface,
    Msg: Message<'a, Opcode = <Conn::Dir as InterfaceDir<I>>::Send, Interface = I>,
    Fut: DriveIo,
{
    type Output = io::Result<()>;
    #[instrument(name = "poll_send", level = "trace", fields(fd = self.fd(), id = self.obj.id.id, msg = format_args!("{}.{}", I::NAME, Msg::NAME), did_send = self.did_send), skip_all, ret(Debug))]
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        unsafe {
            let obj = self.obj;
            let conn = self.obj.conn();
            let msg = self.msg;

            let lock_io = |cx: &mut Context<'_>| match conn.try_lock_io_buf() {
                Some(io) => Poll::Ready(io),
                None => {
                    obj.register_send_locked(cx);
                    Poll::Pending
                }
            };

            if !self.did_send {
                let mut io = ready!(lock_io(cx));

                // The wayland connection was closed, so just hang to make sure error events have
                // the time to get handled.
                if io.interest.contains(Interest::SEND_CLOSED) {
                    trace!("send closed");
                    self.as_mut().get_unchecked_mut().did_send = true;
                    drop(io);
                    obj.wake_sender();
                    return Poll::Pending;
                }

                let (_, mut buf) = 'ret: {
                    if let Some(out) = io.tx_msg_buf(obj.id, msg) {
                        break 'ret out;
                    }

                    ready!(self.drive_io(&mut io, cx))?;
                    if let Some(out) = io.tx_msg_buf(obj.id, msg) {
                        break 'ret out;
                    }

                    obj.register_send(cx);
                    return Poll::Pending;
                };

                msg.write(&mut buf.da, &mut buf.fd)
                    .ok()
                    .expect("serialization error");
                self.as_mut().get_unchecked_mut().did_send = true;
            }

            // if we are the last sender we have to drive the io until it is empty
            if !obj.wake_sender() {
                let mut io = ready!(lock_io(cx));
                if !io.tx.is_empty() {
                    ready!(self.drive_io(&mut io, cx))?;
                }
            } else {
                obj.wake_recver(cx);
            }

            Poll::Ready(Ok(()))
        }
    }
}

impl<Dir> Connection<Dir> {
    pub fn flush(&self) -> Flush<'_, Dir, impl DriveIo> {
        Flush {
            conn: self,
            io_cb: self.drive_io(),
        }
    }
}

pub struct Flush<'a, Dir, Fut> {
    conn: &'a Connection<Dir>,
    io_cb: Fut,
}

impl<'a, Dir, Fut> Future for Flush<'a, Dir, Fut>
where
    Fut: DriveIo,
{
    type Output = io::Result<()>;

    #[instrument(name = "flush", level = "trace", skip(self), ret)]
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        unsafe {
            let s = Pin::into_inner_unchecked(self);
            let conn = s.conn;
            let mut iocb = Pin::new_unchecked(&mut s.io_cb);

            let Some(mut io) = conn.try_lock_io_buf() else {
                s.conn.registry().register_send_locked(cx);
                return Poll::Pending;
            };

            while !io.tx.is_empty() {
                if io.interest.contains(Interest::SEND_CLOSED) {
                    trace!("sending was closed");
                    conn.registry().wake_sender();
                    return Poll::Pending;
                }

                ready!(iocb.as_mut().poll_with_io(&mut io, cx))?;
            }

            Poll::Ready(Ok(()))
        }
    }
}
