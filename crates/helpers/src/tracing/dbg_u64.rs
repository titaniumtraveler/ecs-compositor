use std::{
    fmt::{self, Display},
    ops::RangeInclusive,
};

#[macro_export]
macro_rules! dbg_u64 {
    ($val:expr, $range:expr) => {{
        let val = $val;
        {
            let val = val.to_be_bytes();
            $crate::__reexports::tracing::info!(
                value = %$crate::tracing::dbg_u64::FmtVal::new( 0x00, &val[0..4], $range ),
                value = %$crate::tracing::dbg_u64::FmtVal::new( 0x20, &val[4..8], $range ),
                range = ?$range,
                expr = %stringify!($val),
            );
        }
        val
    }};
    ($val:expr, $range:expr, $($tt:tt)*) => {{
        let val = $val;
        {
            let val = val.to_be_bytes();
            $crate::__reexports::tracing::info!(
                value = %$crate::tracing::dbg_u64::FmtVal::new( 0x00, &val[0..4], $range ),
                value = %$crate::tracing::dbg_u64::FmtVal::new( 0x20, &val[4..8], $range ),
                range = ?$range,
                expr  = %stringify!($val),
                $($tt)*
            );
        }
        val
    }};
}

pub struct FmtVal<'a> {
    pub offset: u8,
    pub val: &'a [u8],
    pub highlight: RangeInclusive<u8>,
}

impl<'a> FmtVal<'a> {
    pub fn new(offset: u8, val: &'a [u8], highlight: RangeInclusive<impl Into<u8>>) -> Self {
        let (start, end) = highlight.into_inner();
        Self { offset, val, highlight: (start.into())..=(end.into()) }
    }
}

impl Display for FmtVal<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let lower = *self.highlight.start();
        let upper = *self.highlight.end();
        let mut color = false;
        for (byte_index, val) in self.val.iter().enumerate() {
            if byte_index != 0 {
                f.write_str(" ")?;
            }

            for bit_index in 0..8 {
                let index = self.offset + (byte_index as u8) * 8 + bit_index;
                if lower == index {
                    color = true;
                }

                {
                    let val = val & (1 << bit_index) >> bit_index;
                    let (color, reset) = match val {
                        _ if !color => ("\x1b[38:2:59:56:50m", "\x1b[m"),
                        0 => ("\x1b[31m", "\x1b[m"),
                        1 => ("\x1b[32m", "\x1b[m"),
                        _ => unreachable!(),
                    };
                    write!(f, "{color}{val}{reset}")?;
                }

                if upper == index {
                    color = false;
                }
            }
        }
        Ok(())
    }
}
