use crate::config::GenerateConfig;
use proc_macro::TokenStream;
use quote::ToTokens;
use syn::parse_macro_input;

mod config;
mod generate;

#[proc_macro]
pub fn protocol(stream: TokenStream) -> TokenStream {
    parse_macro_input!(stream as GenerateConfig)
        .into_token_stream()
        .into()
}
