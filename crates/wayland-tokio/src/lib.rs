#[macro_export]
macro_rules! new_id {
    ($conn:expr, $obj:ident) => {{
        let id;
        (id, $obj) = $conn.new_object();
        id
    }};
}

// mod buffer;
pub mod connection;
mod drive_io;
pub mod handle;
mod msg_io;
