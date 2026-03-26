use ecs_compositor_codegen::builder::{Dir, Wayland};

fn main() {
    let out_dir = &std::env::var("OUT_DIR").unwrap();
    Wayland::protocols(Dir::with("../../wayland-protocols", out_dir).protocol(
        "wayland/protocol/wayland.xml",
        "wayland-protocols/wayland.rs",
    ));
}
