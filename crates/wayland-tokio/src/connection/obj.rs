use crate::handle::{ConnectionHandle, InterfaceDir};
use ecs_compositor_core::{Interface, object};
use std::fmt::Display;

#[derive(Debug)]
pub struct Object<Conn, I>
where
    Conn: ConnectionHandle<Dir: InterfaceDir<I>>,
    I: Interface,
{
    pub(crate) conn: Conn,
    pub(crate) id: object<I>,
}

impl<Conn, I> Object<Conn, I>
where
    Conn: ConnectionHandle<Dir: InterfaceDir<I>>,
    I: Interface,
{
    pub fn id(&self) -> object<I> {
        self.id
    }
}

impl<Conn, I> Display for Object<Conn, I>
where
    Conn: ConnectionHandle<Dir: InterfaceDir<I>>,
    I: Interface,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!(
            "{name}:v{version}#{id}",
            name = I::NAME,
            version = I::VERSION,
            id = self.id.id
        ))
    }
}

impl<Conn, I> Clone for Object<Conn, I>
where
    Conn: ConnectionHandle<Dir: InterfaceDir<I>>,
    I: Interface,
{
    fn clone(&self) -> Self {
        Self {
            conn: self.conn.clone(),
            id: self.id,
        }
    }
}
