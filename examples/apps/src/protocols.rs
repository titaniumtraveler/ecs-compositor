mod interfaces {
    pub use super::{
        wayland::*,
        wlr::{wlr_gamma_control_unstable_v1::*, wlr_layer_shell_unstable_v1::*},
        xdg::xdg_shell::*,
    };
}

pub use ecs_compositor_core as proto;

include!(concat!(env!("OUT_DIR"), "/wayland-protocols/wayland.rs"));

pub mod xdg {
    use super::*;
    include!(concat!(
        env!("OUT_DIR"),
        "/wayland-protocols/xdg/xdg-shell.rs"
    ));
}
pub mod wlr {
    use super::*;

    include!(concat!(
        env!("OUT_DIR"),
        "/wayland-protocols/wlr/wlr-gamma-control-unstable-v1.rs"
    ));

    include!(concat!(
        env!("OUT_DIR"),
        "/wayland-protocols/wlr/wlr-layer-shell-unstable-v1.rs"
    ));
}

include!(concat!(
    env!("OUT_DIR"),
    "/wayland-protocols/brightness/brightness.rs"
));
