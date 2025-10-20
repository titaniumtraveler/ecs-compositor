fn main() {
    let wayland_xml = "../../wayland-protocols/wayland/protocol/wayland.xml";
    ecs_compositor_codegen::protocol(wayland_xml, "wayland-core.rs", true);

    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed={wayland_xml}");
}
