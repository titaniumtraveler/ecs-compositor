#![allow(dead_code)]

use crate::generate::generate_protocol;
use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, TokenStreamExt, quote};
use std::{
    fs::{File, read_to_string},
    io::Write,
    path::{Path, PathBuf},
};
use syn::{LitStr, parse::Parse};
use verbs::Verb;
use wayland_scanner_lib::protocol::Protocol;

pub enum GenerateConfig {
    Include { path: PathBuf, token: LitStr },
    Inline { protocol: Protocol },
    None,
}

mod verbs {
    use proc_macro2::Ident;
    use syn::{LitStr, Token, custom_keyword, parenthesized, parse::Parse, token::As};

    custom_keyword!(include);
    custom_keyword!(generate);

    pub enum Verb {
        Include { xml: LitStr, out: LitStr },
        Generate { xml: LitStr, out: Option<LitStr> },
    }

    impl Parse for Verb {
        fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
            let verb = input.lookahead1();
            input.parse::<Ident>()?;

            let group;
            parenthesized!(group in input);

            if verb.peek(include) {
                Ok(Verb::Include {
                    xml: group.parse()?,
                    out: {
                        group.parse::<Token![as]>()?;
                        group.parse()?
                    },
                })
            } else if verb.peek(generate) {
                Ok(Verb::Generate {
                    xml: group.parse()?,
                    out: {
                        match group.parse::<Option<Token![as]>>()? {
                            Some(As { .. }) => Some(group.parse()?),
                            None => None,
                        }
                    },
                })
            } else {
                Err(verb.error())
            }
        }
    }
}

impl Parse for GenerateConfig {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let verb = input.parse()?;

        match verb {
            Verb::Include { xml, out } => {
                let protocol = read_xml_to_protocol(Path::new(xml.value().as_str()))?;
                write_tokens_to_file(protocol, Path::new(out.value().as_str()), true)?;

                Ok(Self::Include {
                    path: PathBuf::new(), // TODO
                    token: out,
                })
            }
            Verb::Generate { xml, out } => {
                let protocol = read_xml_to_protocol(Path::new(xml.value().as_str()))?;
                match out {
                    None => Ok(Self::Inline { protocol }),
                    Some(out) => {
                        write_tokens_to_file(protocol, Path::new(out.value().as_str()), false)?;
                        Ok(Self::None)
                    }
                }
            }
        }
    }
}

impl ToTokens for GenerateConfig {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        match self {
            GenerateConfig::Include { path, token } => {
                if let Some(path) = path.to_str() {
                    quote! { include!(#path); }.to_tokens(tokens);
                } else {
                    syn::Error::new(
                        token.span(),
                        format_args!(
                            "\
                            Failed to include file path `{path}`.\n\
                            Including non-UTF-8 paths is not supported at this time.\
                            ",
                            path = path.display()
                        ),
                    )
                    .into_compile_error()
                    .to_tokens(tokens)
                }
            }
            GenerateConfig::Inline { protocol } => tokens.append_all(generate_protocol(protocol)),
            GenerateConfig::None => {}
        }
    }
}

pub(crate) fn read_xml_to_protocol(path: &Path) -> syn::Result<Protocol> {
    wayland_scanner_lib::parse::try_parse(
        read_to_string(path)
            .map_err(|err| {
                syn::Error::new(
                    Span::call_site(),
                    format!(
                        "failed to read file {path} with {err}",
                        path = path.display()
                    ),
                )
            })?
            .as_bytes(),
    )
    .map_err(|err| {
        syn::Error::new(
            Span::call_site(),
            format!(
                "failed to parse xml {path} with {err}",
                path = path.display()
            ),
        )
    })
}

pub(crate) fn write_tokens_to_file(
    protocol: Protocol,
    path: &Path,
    formatted: bool,
) -> syn::Result<()> {
    let mut content = {
        let mut tokens = TokenStream::new();
        tokens.append_all(generate_protocol(&protocol));
        tokens.to_string()
    };
    let mut res = Ok(());

    if formatted {
        match syn::parse_file(&content) {
            Ok(file) => content = prettyplease::unparse(&file),
            Err(err) => {
                // std::fmt::Write::write_fmt(&mut content, format_args!("{err:?}")).unwrap();
                res = Err(syn::Error::new(
                    Span::call_site(),
                    format!("failed to reparse file for formatting: {err}"),
                ))
            }
        }
    }

    File::create(path)
        .unwrap()
        .write_all(content.as_bytes())
        .unwrap();

    res
}

fn relative_path(base_dir: impl AsRef<Path>, path: impl AsRef<Path>) -> PathBuf {
    PathBuf::from_iter([base_dir.as_ref(), path.as_ref()])
}
