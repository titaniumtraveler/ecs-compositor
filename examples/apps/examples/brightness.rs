use std::{borrow::Cow, env::VarError, io};
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
        write_to_stdout(rx),
        read_from_stdin(tx),
    )?;

    Ok(())
}

async fn read_from_stdin(mut socket: unix::OwnedWriteHalf) -> io::Result<()> {
    let mut buf = String::new();
    let stdin = &mut BufReader::new(tokio::io::stdin());

    loop {
        let len = stdin.take(4 + 1 + 4 + 1 + 5).read_line(&mut buf).await?;
        match len {
            0 => break Ok(()),
            11.. => break Err(io::Error::other(format!("invalid length `{len}`: `{buf}`"))),
            _ => (),
        }

        let Some((id, bright)) = buf.trim().split_once('=') else {
            eprintln!("could not parse `{}`", buf.trim());
            continue;
        };

        let id = u16::from_str_radix(id, 16)
            .map_err(io::Error::other)?
            .to_le_bytes();
        let bright = u16::from_str_radix(bright, 16)
            .map_err(io::Error::other)?
            .to_le_bytes();

        buf.clear();
        socket
            .write_all(&[id[0], id[1], bright[0], bright[1]])
            .await?;
    }
}

#[allow(clippy::identity_op, clippy::print_with_newline)]
async fn write_to_stdout(mut socket: unix::OwnedReadHalf) -> io::Result<()> {
    let mut buf = [0; 128 * 4];
    let mut read: usize = 0;
    let mut written: usize = 0;
    loop {
        match socket.read(&mut buf).await? {
            0 => {
                break;
            }
            len => {
                written += len;
                while read <= written.wrapping_sub(4) / 4 {
                    let (id, brightness) = (
                        u16::from_le_bytes([buf[read * 4 + 0], buf[read * 4 + 1]]),
                        u16::from_le_bytes([buf[read * 4 + 2], buf[read * 4 + 3]]),
                    );

                    print!("{id:04x}={brightness:04x}\n");
                    read += 1;
                }
            }
        }
    }

    Ok(())
}
