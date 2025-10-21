#[macro_export]
macro_rules! new_id {
    ($conn:expr, $obj:ident) => {{
        let id;
        (id, $obj) = $conn.new_object();
        id
    }};
}

pub mod connection;
pub mod dir;
mod drive_io;
mod msg_io;
mod ready_fut;
mod recv;
mod send;
