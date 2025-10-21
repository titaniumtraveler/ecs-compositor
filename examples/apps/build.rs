fn main() {
    let wayland_core = "../../wayland-protocols/wayland/protocol/wayland.xml";
    ecs_compositor_codegen::protocol(wayland_core, "wayland-core.rs", true);

    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed={wayland_core}");
}
