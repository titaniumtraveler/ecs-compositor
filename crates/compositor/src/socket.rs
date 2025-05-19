use rustix::cmsg_space;
use std::{collections::VecDeque, os::fd::OwnedFd};
use tokio::net::UnixStream;

/// Maximum number of FD that can be sent in a single socket message
pub const MAX_FDS_COUNT: usize = 28;
/// Maximum number of bytes that can be sent in a single socket message
pub const MAX_BYTES: usize = 4096;

const MAX_FD_SPACE: usize = const { cmsg_space!(ScmRights(MAX_FDS_COUNT)) };
const MAX_BYTES_IN: usize = const { MAX_BYTES * 2 }; // Incoming buffer is twice as big to store leftover data
const MAX_BYTES_OUT: usize = const { MAX_BYTES };

pub(crate) struct Socket {
    pub(crate) stream: UnixStream,
    pub(crate) buf_in: Buffer<MAX_BYTES_IN, MAX_FD_SPACE>,
    pub(crate) buf_out: Buffer<MAX_BYTES_OUT, MAX_FD_SPACE>,
}

pub(crate) struct Buffer<const MAX_BYTES: usize, const MAX_FD_SPACE: usize> {
    data: Box<[u8; MAX_BYTES]>,
    ancilary_buf: Box<[u8; MAX_FD_SPACE]>,
    fds: VecDeque<OwnedFd>,
}
