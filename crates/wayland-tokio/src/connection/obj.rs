use crate::{connection::Connection, dir::InterfaceDir};
use ecs_compositor_core::{Interface, object};
use std::{fmt::Display, marker::PhantomData};

#[derive(Debug)]
pub struct Object<Conn, I, Dir>
where
    Conn: AsRef<Connection<Dir>>,
    I: Interface,
    Dir: InterfaceDir<I>,
{
    pub(crate) conn: Conn,
    pub(crate) id: object<I>,
    pub(crate) marker: PhantomData<Dir>,
}

impl<Conn, I, Dir> Display for Object<Conn, I, Dir>
where
    Conn: AsRef<Connection<Dir>>,
    I: Interface,
    Dir: InterfaceDir<I>,
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

impl<Conn, I, Dir> Clone for Object<Conn, I, Dir>
where
    Conn: AsRef<Connection<Dir>> + Clone,
    I: Interface,
    Dir: InterfaceDir<I>,
{
    fn clone(&self) -> Self {
        Self {
            conn: self.conn.clone(),
            id: self.id,
            marker: self.marker,
        }
    }
}
