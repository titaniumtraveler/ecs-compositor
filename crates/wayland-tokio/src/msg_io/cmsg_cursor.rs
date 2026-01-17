use ecs_compositor_core::RawSliceExt;
use libc::{CMSG_DATA, CMSG_FIRSTHDR, CMSG_LEN, CMSG_NXTHDR, c_int, cmsghdr, msghdr};
use std::ptr::{null_mut, slice_from_raw_parts_mut};

pub struct CmsgCursor {
    msg: msghdr,
    hdr: *mut cmsghdr,
    len: usize,
}

impl CmsgCursor {
    pub fn new(msg: msghdr) -> Self {
        unsafe {
            let mut s = Self { hdr: null_mut(), msg, len: 0 };

            s.hdr = CMSG_FIRSTHDR(&s.msg);

            s
        }
    }

    pub fn from_ctrl_buf(ctrl_buf: *mut [u8]) -> Self {
        unsafe {
            let msg = msghdr {
                msg_name: null_mut(),
                msg_namelen: 0,
                msg_iov: null_mut(),
                msg_iovlen: 0,
                msg_control: ctrl_buf.start().cast(),
                msg_controllen: ctrl_buf.len(),
                msg_flags: 0,
            };
            Self::new(msg)
        }
    }

    pub fn write_cursor<'a, T>(
        &'a mut self,
        cmsg_type: c_int,
        cmsg_level: c_int,
    ) -> Result<CmsgCursorWriteData<'a, T>, ()> {
        unsafe {
            if !self.hdr.is_null() {
                (*self.hdr).cmsg_type = cmsg_type;
                (*self.hdr).cmsg_level = cmsg_level;

                let data = RawSliceExt::from_range(
                    CMSG_DATA(self.hdr).cast(),
                    slice_from_raw_parts_mut(self.msg.msg_control.cast(), self.msg.msg_controllen)
                        .end(),
                );

                Ok(CmsgCursorWriteData { cursor: self, data, len: 0 })
            } else {
                Err(())
            }
        }
    }

    #[must_use]
    pub fn read_cmsg(&mut self) -> Option<(cmsghdr, ReadData)> {
        unsafe {
            if self.hdr.is_null() {
                return None;
            }

            let cmsg = *self.hdr;
            let data = RawSliceExt::from_range(
                CMSG_DATA(self.hdr).cast(),
                self.hdr.byte_add(cmsg.cmsg_len).cast(),
            );

            self.hdr = CMSG_NXTHDR(&self.msg, self.hdr);
            Some((cmsg, ReadData { data }))
        }
    }

    pub fn as_slice(&self) -> *mut [u8] {
        slice_from_raw_parts_mut(self.msg.msg_control.cast(), self.len)
    }
}

pub struct ReadData {
    data: *mut [u8],
}

impl ReadData {
    pub fn read_as<T>(self) -> *mut [T] {
        unsafe { <_>::from_range(self.data.start().cast(), self.data.end().cast()) }
    }
}

pub struct CmsgCursorWriteData<'a, T> {
    cursor: &'a mut CmsgCursor,
    data: *mut [T],
    len: usize,
}

impl<'a, T: Copy> CmsgCursorWriteData<'a, T> {
    pub fn write(&mut self, val: T) -> &mut Self {
        unsafe {
            if let Some(buf) = self.data.split_at(1)
                && !self.data.is_null()
            {
                debug_assert!(buf.start().is_aligned());
                buf.start().write(val);
                self.len += 1;
            } else {
                self.data = slice_from_raw_parts_mut(null_mut(), 0);
            }
            self
        }
    }

    pub fn write_unaligned(&mut self, val: T) -> &mut Self {
        unsafe {
            if let Some(buf) = self.data.split_at(1)
                && !self.data.is_null()
            {
                buf.start().write_unaligned(val);
                self.len += 1;
            } else {
                self.data = slice_from_raw_parts_mut(null_mut(), 0);
            }
            self
        }
    }

    pub fn write_slice(&mut self, val: &[T]) -> &mut Self {
        unsafe {
            if let Some(buf) = self.data.split_at(val.len())
                && !self.data.is_null()
            {
                buf.start().copy_from(val.as_ptr(), val.len());
                self.len += val.len();
            } else {
                self.data = slice_from_raw_parts_mut(null_mut(), 0);
            }
            self
        }
    }

    pub fn commit(&mut self) -> Result<usize, usize> {
        unsafe {
            let cursor = &mut self.cursor;
            let len = CMSG_LEN((self.len * size_of::<T>()) as u32) as usize;
            (*cursor.hdr).cmsg_len = len;

            cursor.hdr = CMSG_NXTHDR(&cursor.msg, cursor.hdr);

            if !self.data.is_null() {
                cursor.len += len;
                Ok(len)
            } else {
                Err(len)
            }
        }
    }
}

pub struct CmsgCursorReadData<'a, T> {
    cursor: &'a mut CmsgCursor,
    data: *mut [T],
    len: usize,
}

impl<'a, T: Copy> CmsgCursorReadData<'a, T> {}
