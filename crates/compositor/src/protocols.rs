use ecs_compositor_codegen::protocol;

mod interfaces {
    pub use super::wayland::*;
}

protocol!(include( "wayland-protocols/wayland/protocol/wayland.xml" as "target/wayland.xml.rs"));
