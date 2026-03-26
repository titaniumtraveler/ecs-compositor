pub use ecs_compositor_core as proto;
mod interfaces {
    pub use super::wayland::*;
}

include!(concat!(env!("OUT_DIR"), "/wayland-protocols/wayland.rs"));
