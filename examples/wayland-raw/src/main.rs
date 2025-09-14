use crate::protocols::wayland::{self, wl_display, wl_registry};
use bstr::ByteSlice;
use ecs_compositor_core::{
    Opcode, RawSliceExt, enumeration, message_header, new_id, object,
    primitives::{Result, Value},
    uint,
};
use libc::{__errno_location, AF_UNIX, SOCK_STREAM, close, connect, sockaddr, sockaddr_un, socket};
use std::{
    alloc::{self, Layout},
    env,
    marker::PhantomData,
    mem,
    num::NonZero,
    os::unix::{ffi::OsStrExt, prelude::RawFd},
    path::PathBuf,
    ptr::{self, slice_from_raw_parts, slice_from_raw_parts_mut},
};

mod protocols {
    use ecs_compositor_core as proto;

    mod interfaces {
        pub use super::wayland::*;
    }

    include!(concat!(env!("OUT_DIR"), "/wayland-core.rs"));
}

fn main() {
    unsafe {
        let sock = {
            let path = PathBuf::from_iter([
                env::var_os("XDG_RUNTIME_DIR").unwrap(),
                env::var_os("WAYLAND_DISPLAY").unwrap(),
            ]);

            let sock = socket(AF_UNIX, SOCK_STREAM, 0);
            if sock < 0 {
                // failed creating socket
                return;
            }

            let (addr, len) = {
                let mut addr: libc::sockaddr_un = mem::zeroed();
                addr.sun_family = libc::AF_UNIX as libc::sa_family_t;

                let bytes = path.as_os_str().as_bytes();

                if bytes.contains(&0) {
                    // paths must not contain interior null bytes
                    return;
                }

                if bytes.len() >= addr.sun_path.len() {
                    // path must be shorter than SUN_LEN
                    return;
                }

                ptr::copy_nonoverlapping(
                    bytes.as_ptr(),
                    addr.sun_path.as_mut_ptr().cast(),
                    bytes.len(),
                );

                let mut len = mem::offset_of!(sockaddr_un, sun_path) + bytes.len();
                match bytes.first() {
                    Some(&0) | None => {}
                    Some(_) => len += 1,
                }
                (addr, len as libc::socklen_t)
            };

            #[allow(irrefutable_let_patterns)]
            if let t = connect(sock, &addr as *const sockaddr_un as *const sockaddr, len)
                && t < 0
            {
                return;
            }
            dbg!(sock)
        };

        let buf = {
            const BUF_SIZE: usize = 0x1000;
            let data = alloc::alloc(Layout::from_size_align_unchecked(BUF_SIZE, 4));
            if data.is_null() {
                // allocation error
                return;
            }

            slice_from_raw_parts_mut(data, BUF_SIZE)
        };

        // make a copy
        let mut cursor = buf;
        let mut fds = slice_from_raw_parts_mut(ptr::null_mut(), 0);

        const WL_DISPLAY: u32 = 1;
        const WL_REGISTRY: u32 = 2;
        let bind = wayland::wl_display::request::get_registry {
            registry: new_id {
                id: NonZero::new_unchecked(WL_REGISTRY),
                _marker: PhantomData,
            },
        };

        let header = message_header {
            object_id: object::from_id(NonZero::new_unchecked(1)),
            opcode: 1, // `wl_display::get_registry` is opcode `1`
            datalen: (8 + bind.len()) as u16,
        };

        header.write(&mut cursor, &mut fds).ok().unwrap();
        bind.write(&mut cursor, &mut fds).ok().unwrap();

        cursor = slice_from_raw_parts_mut(
            buf.cast(),
            cursor.cast::<u8>().offset_from_unsigned(buf.cast()),
        );
        loop {
            let res = dbg!(libc::write(sock, cursor.cast(), cursor.len()));
            if res < 0 {
                panic!("io error: {}", *__errno_location());
            }
            cursor.split_at(res as usize).unwrap();

            if cursor.len() == 0 {
                break;
            }
        }

        // reset cursor
        cursor = buf;

        let mut data = {
            let data = cursor;
            let mut len = 0;
            loop {
                let res = libc::read(sock, cursor.cast(), cursor.len());
                if res < 0 {
                    panic!("io error: {}", *__errno_location());
                }
                cursor.split_at(res as usize).unwrap();
                len += res as usize;
                if 8 <= len {
                    break;
                }
            }
            slice_from_raw_parts(data.cast(), len)
        };

        let fds: &mut *const [RawFd] = &mut (&[] as *const [RawFd]);
        let Ok(header) = message_header::read(&mut data, &mut (&[] as *const _)) else {
            panic!("failed reading header");
        };

        println!("header: {header:#?}");

        if data.len() < header.datalen as usize - 8 {
            panic!("not enough data on first read");
        }

        match (header.object_id.id().get(), header.opcode) {
            (WL_DISPLAY, code) if wl_display::event::Opcodes::from_u16(code).is_ok() => {
                use wl_display::event as wl_display;
                match wl_display::Opcodes::from_u16(code).unwrap() {
                    wl_display::Opcodes::error => {
                        let wl_display::error {
                            object_id,
                            code,
                            message,
                        } = wl_display::error::read(&mut data, fds).ok().unwrap();

                        let object_id = object_id.id();
                        let code = wayland::wl_display::enumeration::error::from_u32(code.0);
                        let message = (&*slice_from_raw_parts(
                            message.ptr.unwrap().as_ptr(),
                            message.len.get() as usize,
                        ))
                            .as_bstr();

                        println!(
                            "wl_display::error(object_id: {object_id}, code: {code:?}, message: {message})"
                        )
                    }
                    wl_display::Opcodes::delete_id => todo!(),
                }
            }
            (WL_REGISTRY, code) => {
                use wl_registry::event as wl_registry;
                match wl_registry::Opcodes::from_u16(code)
                    .map_err(|i| format!("wl_registry: unknown opcode {i}"))
                    .unwrap()
                {
                    wl_registry::Opcodes::global => {
                        let wl_registry::global {
                            name,
                            interface,
                            version,
                        } = wl_registry::global::read(&mut data, fds).ok().unwrap();

                        let name = name.0;
                        let interface = (&*slice_from_raw_parts(
                            interface.ptr.unwrap().as_ptr(),
                            interface.len.get() as usize,
                        ))
                            .as_bstr();
                        let version = version.0;

                        println!("wl_registry::global( n: {name} i: {interface} v: {version})");
                    }
                    wl_registry::Opcodes::global_remove => {
                        let wl_registry::global_remove { name } =
                            wl_registry::global_remove::read(&mut data, fds)
                                .ok()
                                .unwrap();

                        let name = name.0;

                        println!("wl_registry::global_remove(name: {name})");
                    }
                }
            }
            _ => panic!("unknown message"),
        }

        close(sock);
    }
}

#[derive(Debug)]
pub struct WaylandHeader {
    pub object_id: object,
    pub datalen: u16,
    pub opcode: u16,
}

impl Value<'_> for WaylandHeader {
    fn len(&self) -> u32 {
        4 + 2 + 2
    }

    unsafe fn read(data: &mut *const [u8], fds: &mut *const [RawFd]) -> Result<Self> {
        unsafe {
            let object_id = object::read(data, fds)?;
            let i = uint::read(data, fds)?.0;

            let datalen = (i >> 16) as u16;
            let opcode = (i & 0xffff) as u16;

            Ok(WaylandHeader {
                object_id,
                datalen,
                opcode,
            })
        }
    }

    unsafe fn write(&self, data: &mut *mut [u8], fds: &mut *mut [RawFd]) -> Result<()> {
        unsafe {
            self.object_id.write(data, fds)?;
            uint((self.datalen as u32) << 16 | self.opcode as u32).write(data, fds)?;
            Ok(())
        }
    }
}
