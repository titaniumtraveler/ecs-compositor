use ecs_compositor_codegen::builder::{Dir, Wayland};

fn main() {
    Wayland::protocols(
        Dir::with(
            "../../wayland-protocols",
            &std::env::var("OUT_DIR").unwrap(),
        )
        .dir(
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
                ),
        ),
    );
}
