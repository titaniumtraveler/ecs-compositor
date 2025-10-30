use ecs_compositor_codegen::builder::{Dir, Wayland};

fn main() {
    Wayland::protocols(
        Dir::new()
            .in_dir("../../wayland-protocols")
            .out_dir(&std::env::var("OUT_DIR").unwrap())
            .protocol("wayland/protocol/wayland.xml", "wayland.rs")
            .dir(
                Dir::new()
                    .in_dir("wlr-protocols/unstable")
                    .out_dir("wlr")
                    .protocol(
                        "wlr-gamma-control-unstable-v1.xml",
                        "wlr-gamma-control-unstable-v1.rs",
                    ),
            ),
    );
}
