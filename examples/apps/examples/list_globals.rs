use apps::protocols::wayland::{wl_display, wl_registry};
use ecs_compositor_tokio::{
    connection::{ClientHandle, Connection},
    handle::Client,
    new_id,
};
use std::sync::Arc;
use tracing::{info, instrument};

#[tokio::main]
async fn main() {
    apps::setup_tracing();
    tokio::spawn(inner()).await.unwrap().unwrap();
}

#[instrument(ret)]
async fn inner() -> anyhow::Result<()> {
    let conn = Arc::new(Connection::<Client>::new()?);

    let display = conn.new_object_with_id(1);
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
        .send(&wl_display::request::get_registry { registry: new_id!(conn, registry) })
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
