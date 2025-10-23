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

    {
        let protocols = ["wlr-gamma-control-unstable-v1"];

        for proto_name in protocols {
            infile.push("../../wayland-protocols/wlr-protocols/unstable");
            infile.push(proto_name);
            infile.set_extension("xml");
            println!("cargo::rerun-if-changed={}", infile.display());

            outfile.push(outdir);
            outfile.push("wlr");
            std::fs::create_dir_all(&outfile).expect("failed creating wlr directory");

            outfile.push(proto_name);
            outfile.set_extension("rs");

            ecs_compositor_codegen::protocol(&infile, &outfile, true);

            infile.clear();
            outfile.clear();
        }
    };
}
