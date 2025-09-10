use crate::config::{read_xml_to_protocol, write_tokens_to_file};
use proc_macro2::Span;
use std::env;
use syn::LitStr;

mod config;
mod generate;

// pub fn protocol(stream: TokenStream) -> TokenStream {
//     parse_macro_input!(stream as GenerateConfig).into_token_stream()
// }

pub fn protocol(protocol: &str, outfile: &str, formatted: bool) {
    fn inner(protocol: &str, outfile: &str, formatted: bool) -> syn::Result<()> {
        let protocol = read_xml_to_protocol(".", &LitStr::new(protocol, Span::call_site()))?;

        write_tokens_to_file(
            protocol,
            env::var_os("OUT_DIR").unwrap(),
            &LitStr::new(outfile, Span::call_site()),
            formatted,
        )?;

        Ok(())
    }

    match inner(protocol, outfile, formatted) {
        Ok(()) => {}
        Err(err) => {
            println!("cargo::error={err}")
        }
    }
}
