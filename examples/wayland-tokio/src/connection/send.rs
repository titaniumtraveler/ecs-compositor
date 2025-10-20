use crate::connection::{Connection, InterfaceDir, Io, Object, ready_fut::DriveIo};
use ecs_compositor_core::{Interface, Message};
use std::{
    fmt::Display,
    future::Future,
    io,
    pin::Pin,
    task::{Context, Poll, ready},
};
use tracing::instrument;

impl<Conn, I, Dir> Object<Conn, I, Dir>
where
    Conn: AsRef<Connection<Dir>>,
    I: Interface,
    Dir: InterfaceDir<I>,
{
    #[instrument(level = "trace", skip(self, msg), fields(%msg))]
    pub async fn send<'a, M>(&'a self, msg: &'a M) -> io::Result<()>
    where
        M: Message<'a, Opcode = Dir::Send, Interface = I> + Display,
    {
        Send {
            obj: self,
            msg,
            ready_fut: self.drive_io(),
            did_send: false,
        }
        .await
    }
}

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct Send<'a, Conn, I, Dir, Msg, Fut>
where
    Conn: AsRef<Connection<Dir>>,
    I: Interface,
    Dir: InterfaceDir<I>,
    Msg: Message<'a, Opcode = Dir::Send, Interface = I>,
    Fut: DriveIo,
{
    obj: &'a Object<Conn, I, Dir>,
    msg: &'a Msg,
    ready_fut: Fut,
    did_send: bool,
}

impl<'a, Conn, I, Dir, Msg, Fut> Send<'a, Conn, I, Dir, Msg, Fut>
where
    Conn: AsRef<Connection<Dir>>,
    I: Interface,
    Dir: InterfaceDir<I>,
    Msg: Message<'a, Opcode = Dir::Send, Interface = I>,
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
}

impl<'a, Conn, I, Dir, Msg, Fut> Future for Send<'a, Conn, I, Dir, Msg, Fut>
where
    Conn: AsRef<Connection<Dir>>,
    I: Interface,
    Dir: InterfaceDir<I>,
    Msg: Message<'a, Opcode = Dir::Send, Interface = I>,
    Fut: DriveIo,
{
    type Output = io::Result<()>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        unsafe {
            let obj = self.obj;
            let conn = self.obj.conn.as_ref();
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
            }

            Poll::Ready(Ok(()))
        }
    }
}
