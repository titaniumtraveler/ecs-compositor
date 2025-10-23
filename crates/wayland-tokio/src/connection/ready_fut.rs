use crate::{
    connection::Connection,
    drive_io::{Interest, Io},
};
use std::{
    future::Future,
    io,
    marker::PhantomData,
    os::unix::net::UnixStream,
    pin::Pin,
    task::{Context, Poll, ready},
};
use tokio::io::unix::{AsyncFd, AsyncFdReadyGuard};
use tracing::{debug, error, instrument, trace};

impl<Dir> Connection<Dir> {
    pub(super) fn drive_io<'a>(&'a self) -> impl DriveIo + 'a {
        AsyncIo {
            f: {
                async |interest| {
                    trace!(?interest, "ready");
                    self.fd.ready(interest).await
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

#[allow(private_interfaces)]
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
    F: FnMut(tokio::io::Interest) -> Fut,
    Fut: Future<Output = io::Result<AsyncFdReadyGuard<'a, UnixStream>>>,
{
    #[instrument(name = "poll_io", level = "trace", ret, skip_all)]
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
                        if !(io.interest & (Interest::RECV_CLOSED | Interest::SEND_CLOSED))
                            .is_empty()
                        {
                            debug!(
                                rx_data_len = io.rx.da.data.len(),
                                rx_ctrl_len = io.rx.fd.data.len(),
                                tx_data_len = io.tx.da.data.len(),
                                tx_ctrl_len = io.tx.fd.data.len(),
                                interest = %io.interest,
                                "Interest is none and recv and/or send is closed. Broken Pipe"
                            );
                            return Poll::Ready(Err(io::Error::new(
                                io::ErrorKind::BrokenPipe,
                                "Connection was closed meanly",
                            )));
                        }

                        error!(interest = %io.interest, "interest should probably **NEVER** be `None` and get polled when interest is not closed");
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
