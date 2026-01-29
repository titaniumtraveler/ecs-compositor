use apps::protocols::brightness;
use bstr::ByteSlice;
use ecs_compositor_core::{Message, RawSliceExt, Value, message_header, object, uint};
use futures::FutureExt;
use std::{borrow::Cow, env::VarError, io, num::NonZero, os::fd::RawFd, ptr};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{UnixStream, unix},
};

#[tokio::main]
async fn main() {
    match run().await {
        Ok(()) => {}
        Err(err) => {
            println!("{err}");
            println!("{err:?}");
        }
    }
}

async fn run() -> io::Result<()> {
    let mut args = std::env::args().skip(1);

    let path: Cow<'_, str> = match args.next() {
        Some(path) => path.into(),
        None => match std::env::var("SOCKET_PATH") {
            Ok(val) => val.into(),
            Err(VarError::NotPresent) => {
                let runtime = std::env::var("XDG_RUNTIME_DIR").expect("`XDG_RUNTIME_DIR` not set");
                format!("{runtime}/wlr-gammastep.sock").into()
            }
            Err(err) => return Err(io::Error::other(err)),
        },
    };
    let path = path.as_ref();

    let (rx, tx) = UnixStream::connect(path).await?.into_split();

    tokio::try_join!(
        biased;
        write_to_stdout(rx).map(ignore_early_eof),
        read_from_stdin(tx),
    )?;

    Ok(())
}

async fn read_from_stdin(mut socket: unix::OwnedWriteHalf) -> io::Result<()> {
    use brightness::output::request::set_config;

    let mut buf = Vec::<u8>::with_capacity(4096);
    let stdin = &mut BufReader::new(tokio::io::stdin());

    loop {
        buf.clear();
        let len = stdin.take("0001=FF00FF00FF00".len() as u64).read_until(b'\n', &mut buf).await?;
        match len {
            0 => break Ok(()),
            11.. => {
                break Err(io::Error::other(format!(
                    "invalid length `{len}`: `{buf}`",
                    buf = buf.as_bstr()
                )));
            }
            _ => (),
        }

        let Some((id, bright)) = buf.trim().as_bstr().split_once_str(b"=") else {
            eprintln!("could not parse `{}`", buf.trim().as_bstr());
            continue;
        };

        fn hex_to_u16(bytes: [&u8; 4]) -> Option<u16> {
            let [c1, c2, c3, c4] = bytes.map(|&c| {
                let h = match c {
                    b'0'..=b'9' => c - b'0' + 0x0,
                    b'a'..=b'f' => c - b'a' + 0xa,
                    b'A'..=b'F' => c - b'A' + 0xa,
                    _ => return None,
                };
                Some(h as u16)
            });

            let val = c1? << 12 | c2? << 8 | c3? << 4 | c4? << 0;
            Some(val)
        }

        macro_rules! hex_to_u16 {
            ($c1:expr) => {
                hex_to_u16([$c1, $c1, $c1, $c1])
            };
            ($c1:expr,$c2:expr) => {
                hex_to_u16([$c1, $c2, $c1, $c2])
            };
            ($c1:expr,$c2:expr,$c3:expr,$c4:expr) => {
                hex_to_u16([$c1, $c2, $c3, $c4])
            };
        }
        let Some([r, g, b]) = (|| match bright.as_bytes() {
            [c1] => Some([hex_to_u16![c1]?; 3]),
            [c1, c2] => Some([hex_to_u16![c1, c2]?; 3]),
            [c1, c2, c3, c4] => Some([hex_to_u16![c1, c2, c3, c4]?; 3]),

            [r1, g1, b1] => Some([hex_to_u16![r1]?, hex_to_u16![g1]?, hex_to_u16![b1]?]),
            [r1, r2, g1, g2, b1, b2] => Some([hex_to_u16![r1, r2]?, hex_to_u16![g1, g2]?, hex_to_u16![b1, b2]?]),
            [r1, r2, r3, r4, g1, g2, g3, g4, b1, b2, b3, b4] => {
                Some([hex_to_u16![r1, r2, r3, r4]?, hex_to_u16![g1, g2, g3, g4]?, hex_to_u16![b1, b2, b3, b4]?])
            }
            _ => None,
        })() else {
            eprintln!("invalid brightness: {bright}", bright = bright.as_bstr());
            eprintln!("brightness needs 1 to 4 hex characters for exactly 1 or 3 color channels");
            eprintln!(
                "brightness length was {len}, but was expected to be one of {expected:?}",
                len = bright.len(),
                expected = [1, 2, 4, 3, 6, 12]
            );
            continue;
        };

        let id = u32::from_str_radix(str::from_utf8(id).map_err(io::Error::other)?, 16).map_err(io::Error::other)?;

        let msg = set_config { red: uint(r as u32), green: uint(g as u32), blue: uint(b as u32) };

        let datalen = message_header::DATA_LEN + msg.len() as u16;
        let hdr = message_header {
            object_id: object::from_id(NonZero::new(id).ok_or_else(|| io::Error::other("id=0 not allowed"))?),
            opcode: set_config::OP,
            datalen,
        };

        unsafe {
            assert!(datalen as usize <= buf.capacity());
            let mut buf: *mut [u8] = ptr::slice_from_raw_parts_mut(buf.as_mut_ptr(), datalen as usize);

            let mut data: *mut [u8] = buf;
            let mut ctrl: *mut [RawFd] = &mut [];

            println!("write buf: {}", buf.len());
            hdr.write(&mut data, &mut ctrl)?;
            msg.write(&mut data, &mut ctrl)?;

            assert!(data.is_empty());
            assert!(ctrl.is_empty());

            buf.set_len(datalen as usize);
            socket.write_all(&*buf).await?;
        }
    }
}

fn ignore_early_eof(res: io::Result<()>) -> io::Result<()> {
    match res {
        Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => Ok(()),
        res => res,
    }
}

#[allow(clippy::identity_op, clippy::print_with_newline)]
async fn write_to_stdout(mut socket: unix::OwnedReadHalf) -> io::Result<()> {
    use apps::protocols::brightness::output::event::change;

    let mut buf = [0; 4096];
    let mut hdr: Option<message_header> = None;

    loop {
        let (id, change { name, red: uint(r), green: uint(g), blue: uint(b) }) = loop {
            match hdr {
                None => unsafe {
                    let datalen = message_header::DATA_LEN as usize;
                    let count = socket.read_exact(&mut buf[..datalen]).await?;
                    assert_eq!(count, datalen);

                    let mut data: *const [u8] = &mut buf[..datalen];
                    let mut ctrl: *const [RawFd] = &[];

                    hdr = Some(message_header::read(&mut data, &mut ctrl)?);

                    debug_assert!(data.is_empty());
                    debug_assert!(ctrl.is_empty());
                },
                Some(hdr) => unsafe {
                    let datalen = hdr.content_len() as usize;
                    let count = socket.read_exact(&mut buf[..datalen]).await?;
                    assert_eq!(count, datalen);

                    let mut data: *const [u8] = &buf[..datalen];
                    let mut ctrl: *const [RawFd] = &[];

                    let msg = change::read(&mut data, &mut ctrl)?;

                    debug_assert_eq!(data.len(), 0);
                    debug_assert_eq!(ctrl.len(), 0);

                    break (hdr.object_id.id().get(), msg);
                },
            }
        };
        hdr = None;
        match name.as_ref().map(|name| name.as_slice_without_trailing_null().as_bstr()) {
            Some(name) => println!("{id:04x}={r:04x}{g:04x}{b:04x}: {name}"),
            _ => println!("{id:04x}={r:04x}{g:04x}{b:04x}"),
        }
    }
}
