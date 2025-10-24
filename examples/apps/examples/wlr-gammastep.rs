use anyhow::anyhow;
use ecs_compositor_core::{Interface, fd, new_id, new_id_dyn, string, uint};
use ecs_compositor_tokio::{
    connection::{ClientHandle, Connection, Object},
    handle::Client,
    new_id,
};
use libc::{MAP_SHARED, MFD_CLOEXEC, PROT_READ, PROT_WRITE};
use protocols::{
    wayland::{wl_display, wl_output, wl_registry},
    wlr::wlr_gamma_control_unstable_v1::{
        zwlr_gamma_control_manager_v1 as gamma_manager, zwlr_gamma_control_v1 as gamma_control,
    },
};
use std::{
    io::{self},
    os::fd::RawFd,
    ptr::null_mut,
    sync::Arc,
};
use tracing::{error, info, instrument};

apps::protocols!();

#[tokio::main]
async fn main() {
    apps::setup_tracing();
    tokio::spawn(inner()).await.unwrap().unwrap()
}

type Conn = Arc<Connection<Client>>;

async fn inner() -> anyhow::Result<()> {
    let conn = Arc::new(Connection::<Client>::new()?);

    let display = conn.new_object_with_id::<wl_display::wl_display>(1);
    tokio::spawn({
        let display = display.clone();
        async move {
            loop {
                let event = display.recv().await?;
                match event.decode_opcode() {
                    wl_display::event::Opcodes::error => {
                        error!(msg = %event.decode_msg::<wl_display::event::error>().ok().unwrap())
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

    match async {
        loop {
            enum Interface {
                Gamma,
                Output,
            }

            let mut gamma_manager = None;
            loop {
                let (name, version, kind) = {
                    let event = registry.recv().await?;
                    match event.decode_opcode() {
                        wl_registry::event::Opcodes::global => {
                            let e: wl_registry::event::global = event.decode_msg().ok().unwrap();
                            match e.interface.as_utf8().map_err(io::Error::other)? {
                                gamma_manager::zwlr_gamma_control_manager_v1::NAME => {
                                    (e.name, e.version, Interface::Gamma)
                                }
                                wl_output::wl_output::NAME => {
                                    (e.name, e.version, Interface::Output)
                                }
                                _unused => {
                                    // debug!(interface = unused, "unused global");
                                    continue;
                                }
                            }
                        }
                        wl_registry::event::Opcodes::global_remove => todo!(),
                    }
                };
                match kind {
                    Interface::Gamma => {
                        let gamma;
                        let bind = bind::<gamma_manager::zwlr_gamma_control_manager_v1>(
                            name,
                            version,
                            "zwlr_gamma_control_manager_v1\0",
                            new_id!(conn, gamma),
                        );
                        registry.send(&bind).await?;
                        gamma_manager = Some(gamma);
                    }
                    Interface::Output => {
                        let output;
                        let bind = bind::<wl_output::wl_output>(
                            name,
                            version,
                            "wl_output\0",
                            new_id!(conn, output),
                        );
                        registry.send(&bind).await?;

                        let gamma_control;
                        gamma_manager
                            .as_ref()
                            .ok_or_else(|| io::Error::other("failed to bind to gamma manager"))?
                            .send(&gamma_manager::request::get_gamma_control {
                                id: new_id!(conn, gamma_control),
                                output: output.id(),
                            })
                            .await?;

                        tokio::spawn(handle_output(gamma_control, output));
                    }
                }
            }
        }
        #[allow(unreachable_code)]
        io::Result::Ok(())
    }
    .await
    {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::BrokenPipe => {
            info!("pipe was broken");
            loop {
                info!("receiving event");
                match registry.recv().await {
                    Ok(ok) => ok.ignore_message(),
                    Err(err) if err.kind() == io::ErrorKind::BrokenPipe => {
                        break Ok(());
                    }
                    Err(err) => break Err(err.into()),
                };
            }
        }
        Err(err) => Err(err.into()),
    }
}

#[instrument(fields(gamma = %gamma_control.id(), output = %output.id()), skip_all, ret)]
async fn handle_output(
    gamma_control: Object<Conn, gamma_control::zwlr_gamma_control_v1>,
    output: Object<Conn, wl_output::wl_output>,
) -> anyhow::Result<()> {
    async fn handle_gamma_event(
        gamma_control: &Object<Conn, gamma_control::zwlr_gamma_control_v1>,
    ) -> anyhow::Result<u32> {
        let err = {
            let event = gamma_control.recv().await?;
            match event.decode_opcode() {
                gamma_control::event::Opcodes::gamma_size => {
                    let m = event
                        .decode_msg::<gamma_control::event::gamma_size>()
                        .ok()
                        .unwrap();
                    info!(%m);
                    return Ok(m.size.0);
                }
                gamma_control::event::Opcodes::failed => {
                    let m = event
                        .decode_msg::<gamma_control::event::failed>()
                        .map_err(|err| anyhow!("{err:?}: {msg}", err = err.err, msg = err.msg))?;
                    error!(%m);
                    anyhow!("{m}")
                }
            }
        };
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        gamma_control
            .send(&gamma_control::request::destroy {})
            .await?;
        Err(err)
    }

    let gamma_event = handle_gamma_event(&gamma_control);
    tokio::pin!(gamma_event);
    let output_event = handle_output_event(&output);
    tokio::pin!(output_event);

    loop {
        info!("`select!()`ing between gamma and output");
        tokio::select! {
            biased;
            // for now do nothing with the output events
            // `wl_output` tends to be WAY busier, so try that first
            res = output_event.as_mut() => {
                output_event.set(handle_output_event(&output));
                res?;
            }
            res = gamma_event.as_mut() => {
                gamma_event.set(handle_gamma_event(&gamma_control));
                let size = res.inspect_err(|err|{ error!("{err}"); })?;
                let gamma_fd = create_gamma_table(size, 0.5)?;
                info!(fd = gamma_fd, "gamma_fd");
                gamma_control
                    .send(&gamma_control::request::set_gamma { fd: fd(gamma_fd) })
                    .await?;

                // ensure the file descriptor was actually sent
                gamma_control.conn().flush().await?;

                unsafe {
                    let ret = libc::close(gamma_fd);
                    info!(ret = ret, "closed");
                    if ret < 0 {
                        return Err(io::Error::last_os_error().into());
                    }
                }
            }
        }
    }
}

fn create_gamma_table(size: u32, brightness: f32) -> io::Result<RawFd> {
    unsafe {
        let table_size = size as usize * size_of::<[u16; 3]>();

        let gamma_fd = libc::memfd_create(c"".as_ptr(), MFD_CLOEXEC);
        if gamma_fd < 0 {
            error!("gamma fd error");
            return Err(io::Error::last_os_error());
        }

        let ret = libc::ftruncate(gamma_fd, table_size as i64);
        if ret < 0 {
            error!("failed truncate");
            return Err(io::Error::last_os_error());
        }

        let data = libc::mmap(
            null_mut(),
            table_size,
            PROT_READ | PROT_WRITE,
            MAP_SHARED,
            gamma_fd,
            0,
        );
        if data.is_null() {
            return Err(io::Error::last_os_error());
        }

        let data = data.cast::<u16>();

        #[allow(clippy::identity_op, clippy::erasing_op)]
        for i in 0..size {
            let brightness = 0x8000;
            let max_brightness: u32 = (1 << 16) - 1;

            let val = ((max_brightness * brightness) >> 16) / size * i;
            let val: u16 = val as u16;

            let size = size as usize;
            let i = i as usize;

            data.add(size * 0 + i).write(val);
            data.add(size * 1 + i).write(val);
            data.add(size * 2 + i).write(val);
        }

        Ok(gamma_fd)
    }
}

fn bind<I: Interface>(
    name: uint,
    version: uint,
    interface: &'static str,
    new_id: new_id<I>,
) -> wl_registry::request::bind<'static> {
    info!(
        name = name.0,
        version= version.0,
        interface = interface,
        new_id= %new_id,
        "binding global"
    );
    assert_eq!(version.0, I::VERSION);
    wl_registry::request::bind {
        name,
        id: new_id_dyn {
            name: string::from_slice(interface.as_bytes()),
            version,
            id: new_id.cast(),
        },
    }
}

async fn handle_output_event(output: &Object<Conn, wl_output::wl_output>) -> io::Result<()> {
    output.recv().await?.ignore_message();
    Ok(())
}
