use std::path::PathBuf;

fn main() {
    let mut infile = PathBuf::new();
    let mut outfile = PathBuf::new();

    let outdir = &PathBuf::from(std::env::var_os("OUT_DIR").expect("missing outdir"));

    println!("cargo::rerun-if-changed=build.rs");
    {
        infile.push("../../wayland-protocols/wayland/protocol/wayland.xml");
        println!("cargo::rerun-if-changed={}", infile.display());

        outfile.push(outdir);
        outfile.push("wayland-core.rs");

        ecs_compositor_codegen::protocol(&infile, &outfile, true);

        infile.clear();
        outfile.clear();
    }
}
