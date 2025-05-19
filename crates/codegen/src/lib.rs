use proc_macro::TokenStream;
use quote::quote;
use std::{ffi::OsString, fs::File, io::Write, path::PathBuf};
use syn::{LitStr, Token, parse::Parse, parse_macro_input};

mod generate;

#[proc_macro]
pub fn protocol(stream: TokenStream) -> TokenStream {
    let path: OsString = parse_macro_input!(stream as LitStr).value().into();
    let path = if let Some(manifest_dir) = std::env::var_os("CARGO_MANIFEST_DIR") {
        let mut buf = PathBuf::from(manifest_dir);
        buf.push(path);
        buf
    } else {
        path.into()
    };
    let file = match std::fs::File::open(&path) {
        Ok(file) => file,
        Err(e) => panic!("Failed to open protocol file {}: {}", path.display(), e),
    };
    let protocol = wayland_scanner_lib::parse::parse(file);
    generate::generate_protocol(&protocol).into()
}

struct WriteToFile {
    xml_path: PathBuf,
    out_file: PathBuf,
}

impl Parse for WriteToFile {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let xml_file = input.parse::<LitStr>()?.value();
        input.parse::<Token![,]>()?;
        let out_file = input.parse::<LitStr>()?.value();

        let (xml_path, out_file) =
            if let Some(manifest_dir) = std::env::var_os("CARGO_WORKSPACE_DIR") {
                (
                    {
                        let mut buf = PathBuf::from(&manifest_dir);
                        buf.push(&xml_file);
                        buf
                    },
                    {
                        let mut buf = PathBuf::from(&manifest_dir);
                        buf.push(&out_file);
                        buf
                    },
                )
            } else {
                (xml_file.as_str().into(), out_file.as_str().into())
            };

        Ok(Self { xml_path, out_file })
    }
}

#[proc_macro]
pub fn protocol_write_to_file(stream: TokenStream) -> TokenStream {
    let config = parse_macro_input!(stream as WriteToFile);
    let file = match std::fs::File::open(&config.xml_path) {
        Ok(file) => file,
        Err(e) => panic!(
            "Failed to open protocol file {}: {}",
            config.xml_path.display(),
            e
        ),
    };
    let protocol = wayland_scanner_lib::parse::parse(file);

    let generated = generate::generate_protocol(&protocol);

    let mut f = File::create(&config.out_file).unwrap();
    f.write_all(generated.to_string().as_bytes()).unwrap();

    let out_file = config.out_file.as_os_str().to_str().unwrap();

    quote! { include!(#out_file) }.into()
}
