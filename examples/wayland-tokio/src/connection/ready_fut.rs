use crate::connection::{Connection, InterfaceDir, Io, Object};
use ecs_compositor_core::Interface;
use std::{
    future::Future,
    io,
    marker::PhantomData,
    os::unix::net::UnixStream,
    pin::Pin,
    task::{Context, Poll, ready},
};
use tokio::io::{
    Interest,
    unix::{AsyncFd, AsyncFdReadyGuard},
};
use tracing::trace;

impl<Conn, I, Dir> Object<Conn, I, Dir>
where
    Conn: AsRef<Connection<Dir>>,
    I: Interface,
    Dir: InterfaceDir<I>,
{
    pub(super) fn drive_io<'a>(&'a self) -> impl DriveIo + 'a {
        AsyncIo {
            f: {
                async |interest| {
                    // tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    trace!(?interest, "ready");
                    self.conn.as_ref().fd.ready(interest).await
                }
            },
            fut: None,
            _marker: PhantomData,
        }
    }
}

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct AsyncIo<'a, F, Fut> {
    f: F,
    fut: Option<Fut>,
    _marker: PhantomData<&'a AsyncFd<UnixStream>>,
}

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub trait DriveIo {
    fn poll_with_io(
        self: Pin<&mut Self>,
        io: &mut Io,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<()>>;
}

impl<'a, F, Fut> DriveIo for AsyncIo<'a, F, Fut>
where
    F: FnMut(Interest) -> Fut,
    Fut: Future<Output = io::Result<AsyncFdReadyGuard<'a, UnixStream>>>,
{
    fn poll_with_io(
        self: Pin<&mut Self>,
        io: &mut Io,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
        unsafe {
            let s = self.get_unchecked_mut();
            let f = &mut s.f;
            let mut fut = Pin::new_unchecked(&mut s.fut);

            match fut.as_mut().as_pin_mut() {
                None => {
                    let Some(interest) = io.query_interest() else {
                        return Poll::Ready(Ok(()));
                    };

                    fut.set(Some(f(interest)));
                    let res = ready!(
                        fut.as_mut()
                            .as_pin_mut()
                            .expect("we just `Pin::set()` it to `Some(_)`")
                            .poll(cx)
                    );
                    fut.set(None);
                    io.drive_io(&mut res?)?;
                    Poll::Ready(Ok(()))
                }
                Some(inner) => {
                    let res = ready!(inner.poll(cx));
                    fut.set(None);
                    io.drive_io(&mut res?)?;
                    Poll::Ready(Ok(()))
                }
            }
        }
    }
}
