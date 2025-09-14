use crate::wl_display::event::error;

pub type Result<T, I = ()> = std::result::Result<T, error<I>>;
