use crate::{
    connection::{Client, ClientHandle, Connection},
    protocols::wayland::{wl_display, wl_registry},
};
use anyhow::Result;
use std::sync::Arc;
use tracing::{info, instrument};
use tracing_subscriber::{EnvFilter, Layer, layer::SubscriberExt, util::SubscriberInitExt};

pub mod connection;

mod protocols {
    mod interfaces {
        pub use super::wayland::*;
    }

    pub use ecs_compositor_core as proto;

    include!(concat!(env!("OUT_DIR"), "/wayland-core.rs"));
}

macro_rules! new_id {
    ($conn:expr, $obj:ident) => {{
        let id;
        (id, $obj) = $conn.new_object();
        id
    }};
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(console_subscriber::spawn())
        .with(
            tracing_subscriber::fmt::layer()
                .pretty()
                .with_filter(EnvFilter::from_default_env()),
        )
        .init();

    tokio::spawn(async { inner().await.unwrap() })
        .await
        .unwrap();
    Ok(())
}

#[instrument(ret)]
async fn inner() -> Result<()> {
    let conn = Arc::new(Connection::<Client>::new()?);

    let display = conn.wl_display();
    tokio::spawn({
        let display = display.clone();

        async move {
            {
                info!("waiting for wl_display");
                let event = display.recv().await?;
                info!("received wl_display");
                match event.decode_opcode() {
                    wl_display::event::Opcodes::error => {
                        info!(msg = %event.decode_msg::<wl_display::event::error>().ok().unwrap())
                    }
                    wl_display::event::Opcodes::delete_id => {
                        info!(msg = %event.decode_msg::<wl_display::event::delete_id>().ok().unwrap())
                    }
                }
            }
            #[allow(unreachable_code)]
            anyhow::Ok(())
        }
    });

    let registry;
    display
        .send(&wl_display::request::get_registry {
            registry: new_id!(conn, registry),
        })
        .await?;

    loop {
        let event = registry.recv().await?;
        match event.decode_opcode() {
            wl_registry::event::Opcodes::global => {
                info!(msg = %event.decode_msg::<wl_registry::event::global>().ok().unwrap());
            }
            wl_registry::event::Opcodes::global_remove => {
                info!(msg = %event.decode_msg::<wl_registry::event::global_remove>().ok().unwrap());
            }
        }
    }
}
