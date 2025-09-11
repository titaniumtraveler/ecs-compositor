fn main() {
    ecs_compositor_codegen::protocol(
        "../../wayland-protocols/wayland/protocol/wayland.xml",
        "wayland-core.rs",
        true,
    );
}
