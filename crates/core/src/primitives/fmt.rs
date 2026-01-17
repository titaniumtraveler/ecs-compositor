use crate::{Interface, array, fd, fixed, int, new_id, new_id_dyn, object, string, uint};
use bstr::ByteSlice;
use std::{
    fmt::{self, Debug, Display, Formatter},
    ptr::slice_from_raw_parts,
};

impl Display for array<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("array {{ len = {len} }}", len = self.len))
    }
}

impl Display for string<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        unsafe {
            f.write_str("string(")?;
            match self.ptr {
                None => write!(f, "<unset string with len of {len}>", len = self.len)?,
                Some(str) => Debug::fmt(
                    (&*slice_from_raw_parts(str.as_ptr(), self.len.get() as usize)).as_bstr(),
                    f,
                )?,
            }
            f.write_str(")")
        }
    }
}

impl string<'_> {
    pub fn fmt_none(f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("{ Option::<String>::None }")
    }
}

impl Display for fd {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl Display for fixed {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl Display for uint {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl Display for int {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let s = self;
        Display::fmt(&s.0, f)
    }
}

impl<I: Interface> Display for object<I> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "object")?;
        if !I::NAME.is_empty() {
            write!(f, "<{NAME}>", NAME = I::NAME)?;
        }
        write!(f, "({id})", id = self.id)?;

        Ok(())
    }
}

impl<I: Interface> object<I> {
    pub fn fmt_none(f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "object")?;
        if !I::NAME.is_empty() {
            write!(f, "<{NAME}>", NAME = I::NAME)?;
        }
        write!(f, "(Null)")?;

        Ok(())
    }
}

impl<I: Interface> Display for new_id<I> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "new_id")?;
        if !I::NAME.is_empty() {
            write!(f, "<{NAME}>", NAME = I::NAME)?;
        }
        write!(f, "({id})", id = self.id)?;

        Ok(())
    }
}

impl Display for new_id_dyn<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let Self { name, version, id } = self;
        write!(f, "new_id {{ name: {name}, version: {version}, id: {id} }}")
    }
}
