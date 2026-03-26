use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use libc::iovec;

pub mod macros;
pub mod recv;
pub mod send;
pub mod span;

pub trait AsyncRecv {
    fn poll_recv(
        self: Pin<&mut Self>,
        data_buf: &mut &mut [iovec],
        ctrl_buf: &mut &mut [iovec],
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<IoCount>>;
}

pub trait AsyncSend {
    fn poll_send(
        self: Pin<&mut Self>,
        data_buf: &mut &mut [iovec],
        ctrl_buf: &mut &mut [iovec],
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<IoCount>>;
}

pub struct IoCount {
    /// bytes read/written
    pub data: usize,
    /// file descriptors read/written
    pub ctrl: usize,
}
