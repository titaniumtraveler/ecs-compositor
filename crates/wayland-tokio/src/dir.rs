use ecs_compositor_core::{Interface, Opcode};

pub trait InterfaceDir<I: Interface> {
    type Recv: Opcode;
    type Send: Opcode;

    fn recv_fd_count(i: u16) -> Option<usize> {
        Self::Recv::from_u16(i).ok().as_ref().map(Opcode::fd_count)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Client;
#[derive(Debug, Clone, Copy)]
pub struct Server;

impl<I: Interface> InterfaceDir<I> for Client {
    type Recv = I::Event;
    type Send = I::Request;
}

impl<I: Interface> InterfaceDir<I> for Server {
    type Recv = I::Request;
    type Send = I::Event;
}
