use anyhow::anyhow;
use apps::{
    bind::str_with_nul,
    protocols::{
        brightness,
        wayland::{wl_display, wl_output, wl_registry},
        wlr::wlr_gamma_control_unstable_v1::{
            zwlr_gamma_control_manager_v1::{self as gamma_manager, zwlr_gamma_control_manager_v1},
            zwlr_gamma_control_v1 as gamma_control,
        },
    },
};
use ecs_compositor_core::{
    Interface, Message, Opcode, RawSliceExt, Value, fd, message_header, new_id, object, primitives::align, string, uint,
};
use ecs_compositor_tokio::{
    connection::{ClientHandle, Connection, Object},
    handle::Client,
    new_id,
};
use futures::{Stream, StreamExt};
use libc::{MAP_SHARED, MFD_CLOEXEC, PROT_READ, PROT_WRITE};
use std::{
    borrow::Cow,
    collections::BTreeMap,
    env::VarError,
    error::Error,
    fmt::Display,
    io,
    num::NonZero,
    os::fd::RawFd,
    pin::{Pin, pin},
    ptr::null_mut,
    sync::{Arc, LazyLock, Mutex},
    task::{Context, Poll, ready},
};
use tokio::{
    io::{AsyncReadExt, AsyncWrite},
    net::{UnixListener, UnixStream},
};
use tokio_stream::wrappers::WatchStream;
use tracing::{debug, error, info, instrument, trace, warn};

#[tokio::main]
async fn main() {
    apps::setup_tracing();
    tokio::try_join!(wayland_client(), config_socket()).unwrap();
}

type Conn = Arc<Connection<Client>>;

static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| Mutex::new(State::default()));

#[derive(Debug, Clone)]
struct OutputState {
    name: Arc<str>,
    brightness: [u16; 3],
}

static UNKNOWN_NAME: LazyLock<Arc<str>> = LazyLock::new(|| Arc::from("<unknown>"));

impl Default for OutputState {
    fn default() -> Self {
        Self { name: Arc::clone(&UNKNOWN_NAME), brightness: [u16::MAX; _] }
    }
}
type OutputSender = tokio::sync::watch::Sender<OutputState>;

#[derive(Default)]
struct State {
    vec: Vec<Option<OutputSender>>,
}

impl State {
    fn new_output(&mut self) -> (usize, WatchStream<OutputState>) {
        let (tx, rx) = tokio::sync::watch::channel(OutputState::default());
        let id;

        match self.vec.iter_mut().enumerate().find(|(_, v)| v.is_none()) {
            Some((entry_id, entry)) => {
                id = entry_id;
                *entry = Some(tx);
            }
            None => {
                id = self.vec.len();
                self.vec.push(Some(tx));
            }
        }

        (id, WatchStream::from_changes(rx))
    }

    fn remove_output(&mut self, id: usize) {
        let sender = &mut self.vec[id];
        assert!(sender.is_some());
        *sender = None;
    }
}

#[instrument(ret)]
async fn config_socket() -> anyhow::Result<()> {
    fn filter_map<T, E: Error>(at: &'static str) -> impl FnMut(Result<T, E>) -> std::future::Ready<Option<T>> {
        move |res| match res {
            Ok(stream) => std::future::ready(Some(stream)),
            Err(err) => {
                warn!(%err, ?err, "error at `{at}`");
                std::future::ready(None)
            }
        }
    }

    let path: Cow<'_, str> = match std::env::var("SOCKET_PATH") {
        Ok(val) => val.into(),
        Err(VarError::NotPresent) => {
            let runtime = std::env::var("XDG_RUNTIME_DIR").expect("`XDG_RUNTIME_DIR` not set");
            format!("{runtime}/wlr-gammastep.sock").into()
        }
        Err(err) => return Err(io::Error::other(err).into()),
    };
    let path = path.as_ref();

    let listener = loop {
        match UnixListener::bind(path) {
            Ok(listener) => break listener,
            Err(err) if err.kind() == io::ErrorKind::AddrInUse => {
                warn!("socket already exists; removing and trying again");
                std::fs::remove_file(path)?;
                continue;
            }
            Err(err) => return Err(err.into()),
        }
    };

    Listener { buf: [0; _], listener, unix_stream: None, written: 0, len: 0 }
        .filter_map(filter_map("socket.accept()"))
        .flat_map_unordered(128, DecodeStream::new)
        .filter_map(filter_map("socket.read()"))
        .for_each_concurrent(1024, async |msg| {
            let DecodedMessage { id, brightness } = msg;
            let state = STATE.lock().unwrap();
            if let 0 = id {
                for (id, sender) in state.vec.iter().enumerate() {
                    match sender {
                        Some(sender) => sender.send_modify(|state| state.brightness = brightness),
                        None => warn!(id, "sender closed"),
                    }
                }
            } else {
                let id = id - 1;
                match state.vec.get(id as usize) {
                    Some(Some(sender)) => sender.send_modify(|state| state.brightness = brightness),
                    Some(None) => warn!(id, "sender closed"),
                    None => warn!(id, "id doesn't exist"),
                }
            }
        })
        .await;

    Ok(())
}

struct Listener {
    buf: [u8; 4096],
    listener: UnixListener,
    unix_stream: Option<UnixStream>,
    written: usize,
    len: usize,
}

#[allow(clippy::identity_op)]
fn write_state_to_buf(buf: &mut [u8; 4096], written: &mut usize, len: &mut usize) -> io::Result<()> {
    *written = 0;
    *len = 0;

    let state = STATE.lock().unwrap();
    for (id, sender) in state
        .vec
        .iter()
        .enumerate()
        .filter_map(|(id, s)| s.as_ref().map(move |s| (id as u16, s)))
    {
        use {brightness::output::event as brightness_output, brightness_output::change};

        #[allow(non_camel_case_types)]
        struct change_from_str<'data> {
            name: Option<str_with_nul<'data>>,
            red: uint,
            green: uint,
            blue: uint,
        }
        impl<'data> Value<'data> for change_from_str<'data> {
            const FDS: usize = 0;
            unsafe fn read(
                _: &mut *const [u8],
                _: &mut *const [RawFd],
            ) -> ecs_compositor_core::primitives::Result<Self> {
                unimplemented!()
            }
            fn len(&self) -> u32 {
                0 + self
                    .name
                    .as_ref()
                    .map(str_with_nul::len)
                    .unwrap_or(Option::<string>::None.len())
                    + self.red.len()
                    + self.green.len()
                    + self.blue.len()
            }
            unsafe fn write(
                &self,
                data: &mut *mut [u8],
                fds: &mut *mut [RawFd],
            ) -> ecs_compositor_core::primitives::Result<()> {
                unsafe {
                    match &self.name {
                        Some(name) => name.write(data, fds)?,
                        None => Option::<string>::None.write(data, fds)?,
                    }
                    self.red.write(data, fds)?;
                    self.green.write(data, fds)?;
                    self.blue.write(data, fds)?;
                    Ok(())
                }
            }
        }

        let id = NonZero::new(id as u32 + 1).unwrap();
        let mut res = Ok(());
        sender.send_if_modified(|OutputState { name, brightness: [red, green, blue] }| {
            res = (|| {
                let msg = change_from_str {
                    name: Some(str_with_nul(name)),
                    red: uint(*red as u32),
                    green: uint(*green as u32),
                    blue: uint(*blue as u32),
                };
                let datalen = message_header::DATA_LEN + msg.len() as u16;
                let hdr = message_header { object_id: object::from_id(id), datalen, opcode: change::OP };

                let mut data: *mut [u8] = &mut buf[*len..*len + datalen as usize];
                let mut ctrl: *mut [RawFd] = &mut [];

                unsafe {
                    hdr.write(&mut data, &mut ctrl)?;
                    msg.write(&mut data, &mut ctrl)?;
                }

                debug_assert!(data.is_empty());
                debug_assert!(ctrl.is_empty());

                trace!(id = id, red, green, blue);

                *len += datalen as usize;

                io::Result::Ok(())
            })();
            false
        });
        res?
    }
    Ok(())
}

impl Stream for Listener {
    type Item = io::Result<UnixStream>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let Listener { buf, listener, unix_stream, written, len } = &mut *self.as_mut();

        let stream = match unix_stream {
            Some(stream) => stream,
            None => {
                let (stream, addr) = ready!(listener.poll_accept(cx))?;
                info!( ?addr, cred = ?stream.peer_cred()?, "accepted stream");
                let stream = unix_stream.insert(stream);

                write_state_to_buf(buf, written, len)?;

                stream
            }
        };
        tokio::pin!(stream);

        while written < len {
            let count = ready!(stream.as_mut().poll_write(cx, &buf[*written..*len]))?;
            debug!(written = written, len = len, count = count, "wrote bytes");
            if let 0 = count {
                return Poll::Ready(Some(Err(io::Error::other("Stream closed early"))));
            }
            *written += count;
        }
        debug!(written, len, "finished writing state to socket");
        ready!(stream.poll_shutdown(cx))?;

        Poll::Ready(Some(Ok(unix_stream.take().unwrap())))
    }
}

struct DecodeStream {
    stream: UnixStream,
    hdr: Option<message_header>,
    buf: [u8; 4096],
    len: u16,
}

impl DecodeStream {
    fn new(stream: UnixStream) -> Self {
        Self { stream, buf: [0; _], len: 0, hdr: None }
    }
}

#[derive(Debug, Clone, Copy)]
struct DecodedMessage {
    id: u32,
    brightness: [u16; 3],
}

impl Stream for DecodeStream {
    type Item = io::Result<DecodedMessage>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let DecodeStream { stream, buf, len, hdr } = &mut *self;

        fn read_exact<'buf>(
            stream: &mut UnixStream,
            buf: &'buf mut [u8; 4096],
            len: &mut u16,
            expected_len: u16,
            cx: &mut Context<'_>,
        ) -> Poll<io::Result<Option<&'buf mut [u8]>>> {
            assert!(expected_len <= 4096);
            while *len < expected_len {
                let buf = &mut buf[(*len as usize)..(expected_len as usize)];
                let res = ready!(pin!(stream.read(buf)).poll(cx))?;

                assert!(res + *len as usize <= expected_len as usize);

                match res {
                    0 => return Poll::Ready(Ok(None)),
                    _ => *len += res as u16,
                }
            }
            Poll::Ready(Ok(Some(&mut buf[..expected_len as usize])))
        }

        use brightness::output::request::set_config;
        let (id, set_config { red: uint(red), green: uint(green), blue: uint(blue) }) = loop {
            match &mut *hdr {
                None => unsafe {
                    let Some(data) = ready!(read_exact(stream, buf, len, message_header::DATA_LEN, cx))? else {
                        return Poll::Ready(None);
                    };
                    let ctrl: &[RawFd] = &[];
                    *hdr = Some(message_header::read(&mut (&*data as _), &mut (ctrl as _))?);
                    *len = 0;
                },
                Some(hdr) => unsafe {
                    use brightness::output::request as brightness_output;

                    let mut data: *const [u8] =
                        match ready!(read_exact(stream, buf, len, message_header::DATA_LEN, cx))? {
                            Some(data) => data,
                            None => return Poll::Ready(None),
                        };
                    let mut ctrl: *const [RawFd] = &[];
                    let opcode = brightness_output::Opcodes::from_u16(hdr.opcode)
                        .map_err(|err| io::Error::other(format!("invalid opcode for brightness: {err}")))?;

                    match opcode {
                        brightness_output::Opcodes::set_config => {
                            let msg = brightness_output::set_config::read(&mut data, &mut ctrl)?;
                            break (hdr.object_id.id().get(), msg);
                        }
                    }
                },
            }
        };

        *len = 0;
        *hdr = None;
        Poll::Ready(Some(Ok(DecodedMessage {
            id,
            brightness: [red as u16, green as u16, blue as u16],
        })))
    }
}

#[instrument(ret)]
async fn wayland_client() -> anyhow::Result<()> {
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
        .send(&wl_display::request::get_registry { registry: new_id!(conn, registry) })
        .await?;

    let mut brightness_map = BTreeMap::<u32, usize>::new();
    match async {
        enum Interface {
            Gamma,
            Output,
        }

        let mut gamma_manager = None;
        loop {
            match loop {
                let event = registry.recv().await?;
                match event.decode_opcode() {
                    wl_registry::event::Opcodes::global => {
                        let e: wl_registry::event::global = event.decode_msg().ok().unwrap();
                        match e.interface.as_utf8().map_err(io::Error::other)? {
                            gamma_manager::zwlr_gamma_control_manager_v1::NAME => {
                                break (e.name, e.version, Interface::Gamma);
                            }
                            wl_output::wl_output::NAME => {
                                break (e.name, e.version, Interface::Output);
                            }
                            unused => {
                                debug!(interface = unused, "unused global");
                                continue;
                            }
                        }
                    }
                    wl_registry::event::Opcodes::global_remove => {
                        let wl_registry::event::global_remove { name: uint(name) } = event.decode_msg().ok().unwrap();
                        let id = brightness_map
                            .get(&name)
                            .expect("expected there to be an extry in brightness_map");
                        STATE.lock().unwrap().remove_output(*id);
                        continue;
                    }
                }
            } {
                (name, version, Interface::Gamma) => {
                    assert!(zwlr_gamma_control_manager_v1::VERSION <= version.0);
                    let gamma;
                    registry.send(&bind { name, id: new_id!(conn, gamma) }).await?;
                    gamma_manager = Some(gamma);
                }
                (name, version, Interface::Output) => {
                    assert!(wl_output::wl_output::VERSION <= version.0);

                    let output;
                    registry.send(&bind { name, id: new_id!(conn, output) }).await?;

                    let gamma_control;
                    gamma_manager
                        .as_ref()
                        .ok_or_else(|| io::Error::other("failed to bind to gamma manager"))?
                        .send(&gamma_manager::request::get_gamma_control {
                            id: new_id!(conn, gamma_control),
                            output: output.id(),
                        })
                        .await?;

                    let (id, brightness) = STATE.lock().unwrap().new_output();
                    brightness_map.insert(name.0, id);
                    tokio::spawn(handle_output(gamma_control, output, brightness));
                }
            }
        }
    }
    .await
    {
        Ok(()) => Ok(()),
        Err(err) if io::Error::kind(&err) == io::ErrorKind::BrokenPipe => {
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
    mut output_state: WatchStream<OutputState>,
) -> anyhow::Result<()> {
    async fn handle_gamma_event(
        gamma_control: &Object<Conn, gamma_control::zwlr_gamma_control_v1>,
    ) -> anyhow::Result<u32> {
        let err = {
            let event = gamma_control.recv().await?;
            match event.decode_opcode() {
                gamma_control::event::Opcodes::gamma_size => {
                    let m = event.decode_msg::<gamma_control::event::gamma_size>().ok().unwrap();
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
        gamma_control.send(&gamma_control::request::destroy {}).await?;
        Err(err)
    }

    async fn set_gamma(
        gamma_control: &Object<Conn, gamma_control::zwlr_gamma_control_v1>,
        brightness: [u16; 3],
        size: u32,
    ) -> io::Result<()> {
        let gamma_fd = create_gamma_table(size, brightness)?;
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
                return Err(io::Error::last_os_error());
            }
        }

        Ok(())
    }

    let gamma_event = handle_gamma_event(&gamma_control);
    tokio::pin!(gamma_event);
    let output_event = handle_output_event(&output);
    tokio::pin!(output_event);
    struct State {
        brightness: Option<[u16; 3]>,
        size: Option<u32>,
    }
    let mut state = State { brightness: None, size: None };

    loop {
        info!("`select!()`ing between gamma and output");
        tokio::select! {
            biased;
            output_state = output_state.next() => {
                let Some(output_state) = output_state else {
                    return Ok(());
                };
                state.brightness = Some(output_state.brightness);

                if let Some(size) = state.size {
                    set_gamma(&gamma_control, output_state.brightness, size).await?;
                }
            }
            // for now do nothing with the output events
            // `wl_output` tends to be WAY busier, so try that first
            res = output_event.as_mut() => {
                output_event.set(handle_output_event(&output));
                res?;
            }
            res = gamma_event.as_mut() => {
                gamma_event.set(handle_gamma_event(&gamma_control));
                let size = res.inspect_err(|err| {
                    error!("{err}");
                })?;
                state.size = Some(size);
                if let Some(brightness) = state.brightness {
                    set_gamma(&gamma_control, brightness, size).await?;
                }
            }
        }
    }
}

fn create_gamma_table(size: u32, [r, g, b]: [u16; 3]) -> io::Result<RawFd> {
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

        unsafe fn write_brightness(data: *mut u16, offset: u32, brightness: u16, size: u32) {
            unsafe {
                for i in 0..size {
                    let brightness = brightness as u32;

                    let val = brightness * i / size;
                    let val: u16 = std::cmp::min(val, u16::MAX as u32) as u16;

                    data.add((offset + i) as usize).write(val);
                }
            }
        }

        write_brightness(data, size * 0, r, size);
        write_brightness(data, size * 1, g, size);
        write_brightness(data, size * 2, b, size);

        Ok(gamma_fd)
    }
}

#[allow(non_camel_case_types)]
struct bind<I: Interface> {
    name: uint,
    id: new_id<I>,
}

impl<'data, I: Interface> Value<'data> for bind<I> {
    const FDS: usize = 0;

    fn len(&self) -> u32 {
        4 // self.name
        + 4 + align::<4>(I::NAME.len() as u32 + 1) // Interface::NAME
        + 4 // Interface::VERSION
        + 4 // self.id
    }

    unsafe fn read(
        _data: &mut *const [u8],
        _fds: &mut *const [RawFd],
    ) -> ecs_compositor_core::primitives::Result<Self> {
        unimplemented!()
    }

    unsafe fn write(
        &self,
        data: &mut *mut [u8],
        fds: &mut *mut [RawFd],
    ) -> ecs_compositor_core::primitives::Result<()> {
        unsafe {
            self.name.write(data, fds)?;

            {
                // Write the interface string to the buffer.
                // Because `Interface::NAME` lacks the expected null terminator,
                // we just pretend we write a string with len+1 to the buffer and then set the
                // padding (which is there *anyways*) to zero, which makes sure we the string data
                // is followed by a null byte. (Which has effectively the same impact as if we
                // wrote a full null terminated string)
                let str_len = I::NAME.len() as u32 + 1;
                uint(str_len).write(data, fds)?;
                let (padding, data) = {
                    let mut padding = data
                        .split_at(align::<4>(str_len) as usize)
                        .expect("not enough space for string");
                    let data = padding.split_at(I::NAME.len()).unwrap();
                    (padding, data)
                };

                data.start().copy_from_nonoverlapping(I::NAME.as_ptr(), I::NAME.len());
                padding.start().write_bytes(0, padding.len());
            }

            uint(I::VERSION).write(data, fds)?;

            self.id.write(data, fds)?;
            Ok(())
        }
    }
}

impl<I: Interface> Display for bind<I> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "new_id_dyn{{ name: {}, id: {}, version: {}}}",
            self.name,
            self.id,
            I::VERSION
        )?;
        Ok(())
    }
}

impl<'data, I: Interface> Message<'data> for bind<I> {
    type Interface = wl_registry::wl_registry;

    const VERSION: u32 = wl_registry::request::bind::VERSION;
    const NAME: &'static str = wl_registry::request::bind::NAME;

    type Opcode = <wl_registry::request::bind<'data> as Message<'data>>::Opcode;

    const OPCODE: Self::Opcode = <wl_registry::request::bind<'data> as Message<'data>>::OPCODE;
    const OP: u16 = <wl_registry::request::bind<'data> as Message<'data>>::OP;
}

async fn handle_output_event(output: &Object<Conn, wl_output::wl_output>) -> io::Result<()> {
    output.recv().await?.ignore_message();
    Ok(())
}
