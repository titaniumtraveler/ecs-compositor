use apps::{
    bind::bind,
    protocols::{
        wayland::{
            wl_buffer, wl_compositor, wl_data_device_manager, wl_display,
            wl_registry::{self, event::global},
            wl_seat,
            wl_shm::{self, enumeration::format},
            wl_shm_pool, wl_surface,
        },
        wlr::wlr_layer_shell_unstable_v1::{zwlr_layer_shell_v1, zwlr_layer_surface_v1},
    },
};
use ecs_compositor_core::{Interface, RawSliceExt, enumeration, int, uint};
use ecs_compositor_tokio::{
    connection::{ClientHandle, Connection, Object},
    handle::Client,
    new_id,
};
use itertools::Itertools;
use libc::{MAP_SHARED, MFD_CLOEXEC, PROT_READ, PROT_WRITE};
// use libc::copy_file_range;
use std::{convert::Infallible, fs::File, io, os::fd::RawFd, ptr, sync::Arc, time::Duration};
use tracing::{debug, error, info, instrument, trace};

fn main() {
    let res = try_main();
    match res {
        Ok(()) => {}
        Err(err) => {
            println!("{err}");
            println!("{err:?}");
        }
    }
}

fn try_main() -> io::Result<()> {
    apps::setup_tracing();
    let data = parse_args()?;
    start_tokio(&data)
}

fn parse_args() -> io::Result<Vec<(String, File)>> {
    std::env::args()
        .skip(1)
        .batching(|iter| Some((iter.next()?, iter.next()?)))
        .map(|(mime, path)| {
            File::open(&path)
                .map(|file| (mime, file))
                .map_err(|err| io::Error::other(format!("{path}: {err}")))
        })
        .collect()
}

#[tokio::main]
async fn start_tokio(data: &[(String, File)]) -> io::Result<()> {
    wayland_client(data).await
}

async fn wayland_client(_data: &[(String, File)]) -> io::Result<()> {
    let conn = Arc::new(Connection::<Client>::new()?);
    let display = conn.new_object_with_id::<wl_display::wl_display>(1);

    let h1 = spawn(handle_display(display.clone()), "wl_display");
    let registry;
    display
        .send(&wl_display::request::get_registry { registry: new_id!(conn, registry) })
        .await?;

    struct Globals<T: GlobalType> {
        seat: T::Type<wl_seat::wl_seat>,
        data_device_manager: T::Type<wl_data_device_manager::wl_data_device_manager>,
        compositor: T::Type<wl_compositor::wl_compositor>,
        layer_shell: T::Type<zwlr_layer_shell_v1::zwlr_layer_shell_v1>,
        wl_shm: T::Type<wl_shm::wl_shm>,
    }

    trait GlobalType {
        type Type<I: Interface>;
    }
    impl GlobalType for uint {
        type Type<I: Interface> = (uint, uint);
    }
    impl<Conn: ClientHandle> GlobalType for Object<Conn, ()> {
        type Type<I: Interface> = Object<Conn, I>;
    }
    impl GlobalType for Option<uint> {
        type Type<I: Interface> = Option<(uint, uint)>;
    }

    impl Default for Globals<Option<uint>> {
        fn default() -> Self {
            Self {
                seat: None,
                data_device_manager: None,
                compositor: None,
                layer_shell: None,
                wl_shm: None,
            }
        }
    }

    impl Globals<uint> {
        async fn do_bind<Conn: ClientHandle, I: Interface>(
            conn: &Conn,
            registry: &Object<Conn, wl_registry::wl_registry>,
            (name, version): (uint, uint),
        ) -> Object<Conn, I> {
            let (id, obj) = conn.new_object();
            let bind = bind { name, id };
            info!(
                bind = %bind,
                version = version.0,
                expected_version = I::VERSION,
                "binding global"
            );
            registry.send(&bind).await.ok().unwrap();
            info!("bound global");
            obj
        }
    }

    let globals = {
        let mut globals = Globals::default();
        loop {
            if let Globals {
                seat: Some(seat),
                data_device_manager: Some(data_device_manager),
                compositor: Some(compositor),
                layer_shell: Some(layer_shell),
                wl_shm: Some(wl_shm),
            } = globals
            {
                break Globals::<uint> {
                    seat,
                    data_device_manager,
                    compositor,
                    layer_shell,
                    wl_shm,
                };
            }

            let event = registry.recv().await?;
            match event.decode_opcode() {
                wl_registry::event::Opcodes::global => {
                    let global: global = event.decode_msg().ok().unwrap();
                    use {
                        wl_compositor::wl_compositor,
                        wl_data_device_manager::wl_data_device_manager, wl_seat::wl_seat,
                        wl_shm::wl_shm, zwlr_layer_shell_v1::zwlr_layer_shell_v1,
                    };

                    match global.interface.as_utf8().map_err(io::Error::other)? {
                        wl_seat::NAME => global.bind(&mut globals.seat),
                        wl_data_device_manager::NAME => {
                            global.bind(&mut globals.data_device_manager)
                        }
                        wl_compositor::NAME => global.bind(&mut globals.compositor),
                        zwlr_layer_shell_v1::NAME => global.bind(&mut globals.layer_shell),
                        wl_shm::NAME => global.bind(&mut globals.wl_shm),
                        _ => continue,
                    }
                }
                wl_registry::event::Opcodes::global_remove => todo!(),
            }
        }
    };

    let h2 = spawn(handle_registry(Object::clone(&registry)), "wl_registry");

    {
        use {
            wl_compositor::request as wl_compositor, wl_surface::request as wl_surface,
            zwlr_layer_shell_v1::request as wlr_layer_shell,
            zwlr_layer_surface_v1::enumeration::anchor,
            zwlr_layer_surface_v1::request as wlr_layer_surface,
        };

        let compositor = Globals::do_bind(&conn, &registry, globals.compositor).await;
        let layer_shell = Globals::do_bind(&conn, &registry, globals.layer_shell).await;
        let wl_shm = Globals::do_bind(&conn, &registry, globals.wl_shm).await;
        let h4 = spawn(handle_wl_shm(wl_shm.clone()), "wl_shm");

        let surface;
        compositor
            .send(&wl_compositor::create_surface { id: new_id!(conn, surface) })
            .await?;
        let h3 = spawn(handle_surface(surface.clone()), "wl_surface");

        let layer_surface;
        layer_shell
            .send(&wlr_layer_shell::get_layer_surface {
                id: new_id!(conn, layer_surface),
                surface: surface.id(),
                output: None,
                layer: zwlr_layer_shell_v1::enumeration::layer::overlay.to_uint(),
                namespace: ecs_compositor_core::string::from_slice(b"drag-and-drop\0"),
            })
            .await?;

        layer_surface
            .send(&wlr_layer_surface::set_anchor {
                anchor: (anchor::top | anchor::left | anchor::bottom | anchor::right).to_uint(),
            })
            .await?;

        layer_surface
            .send(&wlr_layer_surface::set_keyboard_interactivity {
                keyboard_interactivity:
                    zwlr_layer_surface_v1::enumeration::keyboard_interactivity::exclusive.to_uint(),
            })
            .await?;

        surface.send(&wl_surface::commit {}).await?;

        let configure = {
            use zwlr_layer_surface_v1::event as wlr_layer_surface;
            use zwlr_layer_surface_v1::event::Opcodes::*;
            let event = layer_surface.recv().await?;
            match event.decode_opcode() {
                configure => {
                    let event = event
                        .decode_msg::<wlr_layer_surface::configure>()
                        .ok()
                        .unwrap();
                    info!(event =  %event);
                    event
                }
                closed => {
                    info!(event =  %event.decode_msg::<wlr_layer_surface::closed>().ok().unwrap());
                    return Err(io::Error::other("closed"));
                }
            }
        };

        let buf = memfd_buffer::new(
            &conn,
            &wl_shm,
            BufSize { width: configure.width.0, height: configure.height.0, scale: 2 },
        )
        .await?;
        buf.render_to_fd(0x00_00_00_00)?;
        let h6 = spawn(handle_wl_buffer(buf.buffer.clone()), "wl_buffer");
        info!(size = ?buf.size, "buffer");

        surface
            .send(&wl_surface::attach { buffer: Some(buf.buffer.id()), x: int(0), y: int(0) })
            .await?;
        surface
            .send(&wl_surface::damage_buffer {
                x: int(0),
                y: int(0),
                width: int(buf.size.actual_width()),
                height: int(buf.size.actual_height()),
            })
            .await?;
        layer_surface
            .send(&wlr_layer_surface::ack_configure { serial: configure.serial })
            .await?;
        surface.send(&wl_surface::commit {}).await?;

        let h5 = spawn(handle_layer_surface(layer_surface), "wlr_layer_surface");

        error!("todo");

        tokio::try_join!(h1, h2, h3, h4, h5, h6, timeout(Duration::from_secs(5))).map(|_| ())
    }
}

async fn timeout(dur: Duration) -> io::Result<()> {
    info!(?dur, "starting timeout");
    tokio::time::sleep(dur).await;
    Err(io::Error::other("timeout reached"))
}

fn spawn<T: Send + 'static>(
    fut: impl Future<Output = T> + 'static + Send,
    spawn: &'static str,
) -> impl Future<Output = io::Result<T>> {
    info!(spawn);
    let handle = tokio::spawn(fut);
    async { handle.await.map_err(io::Error::other) }
}

#[instrument(level = "debug", fields(wl_display = %wl_display.id()),skip_all)]
async fn handle_display(wl_display: Object<Arc<Connection<Client>>, wl_display::wl_display>) {
    use wl_display::event as wl_display;
    let Err(err): io::Result<Infallible> = async {
        loop {
            let event = wl_display.recv().await?;
            match event.decode_opcode() {
                wl_display::Opcodes::error => {
                    error!(msg = %event.decode_msg::<wl_display::error>().ok().unwrap())
                }
                wl_display::Opcodes::delete_id => {
                    info!(msg = %event.decode_msg::<wl_display::delete_id>().ok().unwrap())
                }
            }
        }
    }
    .await;
    error!(%err, "display errored");
}

#[instrument(level = "debug", fields(wl_registry = %registry.id()), skip_all)]
async fn handle_registry<Conn: ClientHandle>(registry: Object<Conn, wl_registry::wl_registry>) {
    debug!("start handling registry");
    let Err(err): io::Result<Infallible> = async {
        loop {
            let event = registry.recv().await?;
            match event.decode_opcode() {
                wl_registry::event::Opcodes::global => {
                    trace!(event = %event.decode_msg::<wl_registry::event::global>().ok().unwrap());
                }
                wl_registry::event::Opcodes::global_remove => {
                    trace!(event = %event.decode_msg::<wl_registry::event::global_remove>().ok().unwrap());
                }
            }
        }
    }
    .await;
    error!(%err, "registry errored");
}

#[instrument(level = "debug", fields(wl_surface = %surface.id()), skip_all)]
async fn handle_surface<Conn: ClientHandle>(surface: Object<Conn, wl_surface::wl_surface>) {
    debug!("start handling surface");
    let Err(err): io::Result<Infallible> = async {
        use wl_surface::event::Opcodes::*;
        loop {
            use wl_surface::event as wl_surface;
            let event = surface.recv().await?;
            match event.decode_opcode() {
                enter => {
                    info!(event = %event.decode_msg::<wl_surface::enter>().ok().unwrap())
                }
                leave => {
                    info!(event = %event.decode_msg::<wl_surface::leave>().ok().unwrap())
                }
                preferred_buffer_scale => {
                    info!(event = %event.decode_msg::<wl_surface::preferred_buffer_scale>().ok().unwrap())
                }
                preferred_buffer_transform => {
                    info!(event = %event.decode_msg::<wl_surface::preferred_buffer_transform>().ok().unwrap())
                }
            }
        }
    }
    .await;
    error!(%err, "surface errored");
}

#[instrument(level = "debug", fields(wlr_layer_surface = %layer_surface.id()), skip_all)]
async fn handle_layer_surface<Conn: ClientHandle>(
    layer_surface: Object<Conn, zwlr_layer_surface_v1::zwlr_layer_surface_v1>,
) {
    debug!("start handling layer_surface");
    let Err(err): io::Result<Infallible> = async {
        use zwlr_layer_surface_v1::event::Opcodes::*;
        loop {
            use zwlr_layer_surface_v1::event as wlr_layer_surface;
            let event = layer_surface.recv().await?;
            match event.decode_opcode() {
                configure => {
                    info!(event = %event.decode_msg::<wlr_layer_surface::configure>().ok().unwrap())
                }
                closed => {
                    info!(event = %event.decode_msg::<wlr_layer_surface::closed>().ok().unwrap())
                }
            }
        }
    }
    .await;
    error!(%err, "layer_surface errored");
}

#[instrument(level = "debug", fields(wl_shm = %wl_shm.id()), skip_all)]
async fn handle_wl_shm<Conn: ClientHandle>(wl_shm: Object<Conn, wl_shm::wl_shm>) {
    debug!("start handling wl_shm");
    let Err(err): io::Result<Infallible> = async {
        use wl_shm::event::Opcodes::*;
        loop {
            let event = wl_shm.recv().await?;
            match event.decode_opcode() {
                format => {
                    let event = event.decode_msg::<wl_shm::event::format>().ok().unwrap();
                    let pixel_format =
                        wl_shm::enumeration::format::from_u32(event.format.0).unwrap();
                    info!(pixel_format = ?pixel_format, %event);
                }
            }
        }
    }
    .await;
    error!(%err, "wl_shm errored");
}

#[allow(non_camel_case_types)]
struct memfd_buffer<Conn: ClientHandle> {
    fd: RawFd,
    pool: Object<Conn, wl_shm_pool::wl_shm_pool>,
    buffer: Object<Conn, wl_buffer::wl_buffer>,
    size: BufSize,
}

#[derive(Debug, Clone, Copy)]
struct BufSize {
    width: u32,
    height: u32,
    scale: u32,
}

impl BufSize {
    fn in_pixels(self) -> u32 {
        let BufSize { width, height, scale } = self;
        width * scale * height * scale
    }
    fn in_bytes(self) -> u32 {
        self.in_pixels() * (size_of::<u32>() as u32)
    }

    fn actual_width(self) -> i32 {
        (self.width * self.scale) as i32
    }

    fn actual_height(self) -> i32 {
        (self.height * self.scale) as i32
    }
}

impl<Conn: ClientHandle> memfd_buffer<Conn> {
    async fn new(
        conn: &Conn,
        wl_shm: &Object<Conn, wl_shm::wl_shm>,
        size: BufSize,
    ) -> io::Result<memfd_buffer<Conn>> {
        use {wl_shm::request as wl_shm, wl_shm_pool::request as wl_shm_pool};

        let fd = unsafe {
            let fd = libc::memfd_create(c"".as_ptr(), MFD_CLOEXEC);
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            if libc::ftruncate(fd, size.in_bytes() as i64) < 0 {
                return Err(io::Error::last_os_error());
            }
            fd
        };

        let pool;
        wl_shm
            .send(&wl_shm::create_pool {
                id: new_id!(conn, pool),
                fd: ecs_compositor_core::fd(fd),
                size: int(size.in_bytes() as i32),
            })
            .await?;
        let buffer;
        pool.send(&wl_shm_pool::create_buffer {
            id: new_id!(conn, buffer),
            offset: int(0),
            width: int(size.actual_width()),
            height: int(size.actual_height()),
            stride: int(size.actual_width() * (size_of::<u32>() as i32)),
            format: format::argb8888.to_uint(),
        })
        .await?;

        Ok(memfd_buffer { fd, pool, buffer, size })
    }

    fn map_buf(&self) -> io::Result<*mut [u32]> {
        unsafe {
            let len = self.size.in_bytes() as usize;
            let addr = libc::mmap(
                ptr::null_mut(),
                len,
                PROT_READ | PROT_WRITE,
                MAP_SHARED,
                self.fd,
                0,
            );

            if addr.is_null() {
                return Err(io::Error::last_os_error());
            }
            let buf =
                ptr::slice_from_raw_parts_mut(addr.cast::<u32>(), self.size.in_pixels() as usize);
            info!(?buf, "mapped buf");

            Ok(buf)
        }
    }

    fn unmap_buf(&self, buf: *mut [u32]) -> io::Result<()> {
        unsafe {
            let len = self.size.in_bytes() as usize;
            if libc::munmap(buf.cast(), len) < 0 {
                return Err(io::Error::last_os_error());
            }
            info!(?buf, "unmapped buf");

            Ok(())
        }
    }

    fn render_to_fd(&self, pixel_value: u32) -> io::Result<()> {
        unsafe {
            let buf = self.map_buf()?;
            let data = buf.start();

            info!("start writing buffer");
            for offset in 0..buf.len() {
                data.add(offset).write(pixel_value);
            }
            info!("finished writing buffer");

            self.unmap_buf(buf)?;
            Ok(())
        }
    }
}

#[instrument(level = "debug", fields(wl_buffer = %wl_buffer.id()), skip_all)]
async fn handle_wl_buffer<Conn: ClientHandle>(wl_buffer: Object<Conn, wl_buffer::wl_buffer>) {
    debug!("start handling wl_buffer");
    let Err(err): io::Result<Infallible> = async {
        use wl_buffer::event::Opcodes::*;
        loop {
            use wl_buffer::event as wl_buffer;
            let event = wl_buffer.recv().await?;
            match event.decode_opcode() {
                release => {
                    let event = event.decode_msg::<wl_buffer::release>().ok().unwrap();
                    info!(%event, "release")
                }
            }
        }
    }
    .await;
    error!(%err, "wl_buffer errored");
}
