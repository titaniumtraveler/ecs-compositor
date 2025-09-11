use crate::protocols::wayland::{
    self,
    wl_registry::{
        self,
        events::{Global, GlobalRemove},
    },
};
use bstr::ByteSlice;
use ecs_compositor_core::{
    Message, NewId,
    primitives::{Object, Primitive, Result, ThickPtr, UInt},
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
    ptr::{self, NonNull},
};

mod protocols {
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
            let Some(data) = NonNull::new(ptr::slice_from_raw_parts_mut(data, BUF_SIZE)) else {
                // allocation error
                return;
            };
            data
        };

        let mut cursor = ThickPtr {
            ptr: buf.cast(),
            len: buf.len(),
        };

        let mut fds = ThickPtr {
            // We currently don't support reading/writing FDs
            ptr: NonNull::dangling(),
            len: 0,
        };

        const WL_REGISTRY: u32 = 2;
        let bind = wayland::wl_display::requests::GetRegistry {
            registry: NewId {
                id: NonZero::new_unchecked(WL_REGISTRY),
                _marker: PhantomData,
            },
        };

        let header = WaylandHeader {
            object_id: Object::from_id(NonZero::new_unchecked(1)),
            opcode: 1, // `wl_display::get_registry` is opcode `1`
            datalen: (8 + bind.write_len()) as u16,
        };

        header.write(&mut cursor, &mut fds).ok().unwrap();
        bind.write(&mut cursor, &mut fds).ok().unwrap();

        // set cursor to the range that we just wrote to
        cursor.len = cursor.ptr.byte_offset_from_unsigned(buf);
        cursor.ptr = buf.cast();

        loop {
            let res = dbg!(libc::write(sock, cursor.ptr.as_ptr().cast(), cursor.len));
            if res < 0 {
                panic!("io error: {}", *__errno_location());
            }
            cursor.advance(res as usize);

            if cursor.len == 0 {
                break;
            }
        }

        // reset cursor
        cursor.ptr = buf.cast();
        cursor.len = buf.len();

        let mut data = {
            let data = cursor.ptr;
            let mut len = 0;
            loop {
                let res = libc::read(sock, cursor.ptr.as_ptr() as *mut _, cursor.len);
                if res < 0 {
                    panic!("io error: {}", *__errno_location());
                }
                cursor.advance(res as usize);
                len += res as usize;
                if WaylandHeader::write_len() as usize <= len {
                    break;
                }
            }
            &*ptr::slice_from_raw_parts(data.as_ptr(), len)
        };

        let mut fds: &[RawFd] = &[];
        let Ok(header) = WaylandHeader::read(&mut data, &mut fds) else {
            panic!("failed reading header");
        };

        println!("header: {header:#?}");

        let true = (header.object_id.id().get() == 2 && [0, 1].contains(&header.opcode)) else {
            panic!("invalid event")
        };

        if data.len() < header.datalen as usize - 8 {
            panic!("not enough data on first read");
        }

        match (header.object_id.id().get(), header.opcode) {
            (WL_REGISTRY, 0) => {
                let Global {
                    name,
                    interface,
                    version,
                } = wl_registry::events::Global::read(data, &[]).ok().unwrap();

                let name = name.0;
                let interface = {
                    &*ptr::slice_from_raw_parts(
                        interface.ptr.unwrap().as_ptr(),
                        interface.len.get() as usize,
                    )
                }
                .as_bstr();
                let version = version.0;

                println!(
                    "wl_registry::global(name: {name}, interface: {interface}, version: {version})"
                );
            }
            (WL_REGISTRY, 1) => {
                let GlobalRemove { name } = wl_registry::events::GlobalRemove::read(data, &[])
                    .ok()
                    .unwrap();
                let name = name.0;

                println!("wl_registry::global_remove(name: {name})");
            }
            _ => panic!("unknown message"),
        }

        close(sock);
    }
}

#[derive(Debug)]
pub struct WaylandHeader {
    pub object_id: Object,
    pub datalen: u16,
    pub opcode: u16,
}

impl<'data> WaylandHeader {
    fn read(data: &mut &'data [u8], fds: &mut &[RawFd]) -> Result<Self> {
        let object_id = Object::read(data, fds)?;

        let int = UInt::read(data, fds)?.0;

        let datalen = (int >> 16) as u16;
        let opcode = (int & 0xffff) as u16;

        Ok(Self {
            object_id,
            datalen,
            opcode,
        })
    }

    fn write_len() -> u32 {
        4 + 2 + 2
    }

    fn write(&self, data: &mut ThickPtr<u8>, fds: &mut ThickPtr<RawFd>) -> Result<()> {
        self.object_id.write(data, fds)?;
        UInt((self.datalen as u32) << 16 | self.opcode as u32).write(data, fds)?;
        Ok(())
    }
}
