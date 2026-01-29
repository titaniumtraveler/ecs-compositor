use ecs_compositor_codegen::builder::{Dir, Wayland};

fn main() {
    let out_dir = &std::env::var("OUT_DIR").unwrap();
    Wayland::protocols(
        Dir::with("../../wayland-protocols", out_dir).dir(
            Dir::with("", "wayland-protocols")
                .dir(
                    Dir::new()
                        .protocol("wayland/protocol/wayland.xml", "wayland.rs")
                        .dir(
                            Dir::with("wayland-protocols/stable", "xdg")
                                .protocol("xdg-shell/xdg-shell.xml", "xdg-shell.rs"),
                        ),
                )
                .dir(
                    Dir::with("wlr-protocols/unstable", "wlr")
                        .protocol(
                            "wlr-gamma-control-unstable-v1.xml",
                            "wlr-gamma-control-unstable-v1.rs",
                        )
                        .protocol(
                            "wlr-layer-shell-unstable-v1.xml",
                            "wlr-layer-shell-unstable-v1.rs",
                        ),
                )
                .dir(
                    Dir::with(env!("CARGO_MANIFEST_DIR"), "brightness")
                        .protocol("./resources/brightness.xml", "brightness.rs"),
                ),
        ),
    );
}
