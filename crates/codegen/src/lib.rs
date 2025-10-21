use crate::config::{read_xml_to_protocol, write_tokens_to_file};
use std::path::Path;

mod config;
mod generate;

// pub fn protocol(stream: TokenStream) -> TokenStream {
//     parse_macro_input!(stream as GenerateConfig).into_token_stream()
// }

pub fn protocol(protocol: impl AsRef<Path>, outfile: impl AsRef<Path>, formatted: bool) {
    fn inner(infile: &Path, outfile: &Path, formatted: bool) -> syn::Result<()> {
        write_tokens_to_file(read_xml_to_protocol(infile)?, outfile, formatted)?;

        Ok(())
    }

    match inner(protocol.as_ref(), outfile.as_ref(), formatted) {
        Ok(()) => {}
        Err(err) => {
            println!("cargo::error={err}")
        }
    }
}
