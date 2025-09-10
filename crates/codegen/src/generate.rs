use crate::generate::flat_map_fn::IteratorExt;
use proc_macro2::{Literal, TokenStream};
use quote::{ToTokens, format_ident, quote};
use std::fmt::Write;
use wayland_scanner_lib::protocol::{Arg, Entry, Enum, Interface, Message, Protocol, Type};

mod flat_map_fn;

pub fn generate_protocol(protocol: &Protocol) -> TokenStream {
    let Protocol {
        name,
        description,
        interfaces,
        ..
    } = protocol;

    let docs = Docs::Global.description(description);
    let name = mod_name(name);
    let interfaces = interfaces.iter().map(generate_interface);
    quote! {
        #[allow(unused_variables,unused_mut,unused_imports, dead_code)]
        #[allow(clippy::doc_lazy_continuation,clippy::identity_op)]
        pub mod #name {
            #docs
            #(#interfaces)*
        }
    }
}

fn generate_interface(interface: &Interface) -> TokenStream {
    let Interface {
        name,
        version,
        description,
        requests,
        events,
        enums,
    } = interface;

    let error = if let Some(error) = enums.iter().find(|e| e.name == "error") {
        let name = typ_name(&error.name);
        quote! {enums::#name}
    } else {
        quote! {u32}
    };

    let typ_name = typ_name(name);
    let mod_name = mod_name(name);

    let docs = Docs::Global.description(description);

    let iface_name = {
        let version = Literal::u32_unsuffixed(*version);

        quote! {
            use {
                super::super::interfaces::*,
                std::os::fd::RawFd,
                ecs_compositor_core::*,
            };

            pub enum #typ_name {}
            impl Interface for #typ_name {
                const NAME:   &str = #name;
                const VERSION: u32 = #version;

                type Error         = #error;
            }
        }
    };

    let requests = {
        let requests = requests.iter().map(|msg| generate_message(msg, &typ_name));
        quote! {
            pub mod requests {
                use super::*;
                #(#requests)*
            }
        }
    };
    let events = {
        let events = events.iter().map(|msg| generate_message(msg, &typ_name));
        quote! {
            pub mod events {
                use super::*;
                #(#events)*
            }
        }
    };
    let enums = {
        let enums = enums.iter().map(generate_enum);
        quote! {
            pub mod enums {
                use super::*;
                #(#enums)*
            }
        }
    };

    quote! {
        pub mod #mod_name {
            #docs

            #iface_name

            #requests
            #events
            #enums
        }
    }
}

fn generate_message(message: &Message, iface_name: &syn::Ident) -> TokenStream {
    let Message {
        name,
        typ: _,
        since: _,
        description,
        args,
    } = message;

    let name = typ_name(name);

    let lifetime = if message.args.iter().any(|arg| {
        matches!(arg.typ, Type::Array | Type::String | Type::NewId if arg.interface.is_none())
    }) {
        quote! {<'data>}
    } else {
        quote! {}
    };

    let item = {
        let docs = Docs::Local.description(description);

        let fields = args.iter().map(gen_field);

        quote! {
            #docs
            pub struct #name #lifetime {
                #(#fields)*
            }
        }
    };

    let impl_message = {
        let fd_count = Literal::usize_unsuffixed(
            args.iter()
                .filter(|arg| matches!(arg.typ, Type::Fd))
                .count(),
        );

        let fields_read = args.iter().map(|arg| {
            let name = mod_name(&arg.name);
            let typ = match arg.typ {
                Type::NewId if arg.interface.is_none() => quote! { NewIdDyn },

                Type::Int => quote! {    Int    },
                Type::Uint => quote! {   UInt   },
                Type::Fixed => quote! {  Fixed  },
                Type::String => quote! { String },
                Type::Object => quote! { Object },
                Type::NewId => quote! {  NewId  },
                Type::Array => quote! {  Array  },
                Type::Fd => quote! {     Fd     },
                Type::Destructor => unreachable!(),
            };
            quote! {
                #name: #typ::read(&mut data, &mut fds)?,
            }
        });

        let fields_write_len = args.iter().map(|arg| {
            let name = mod_name(&arg.name);
            quote! {
                + self.#name.len()
            }
        });

        let fields_write = args.iter().map(|arg| {
            let name = mod_name(&arg.name);
            quote! {
                self.#name.write(data,fds)?;
            }
        });

        quote! {
            impl<'data> Message<'data,#fd_count,#iface_name> for #name #lifetime {
                fn read(mut data: &'data [u8], fds: &[RawFd; #fd_count]) -> primitives::Result<Self> {
                    let mut fds = fds.as_slice();
                    Ok(Self {
                        #(#fields_read)*
                    })
                }

                fn write_len(&self) -> u32 {
                    0 #(#fields_write_len)*
                }

               fn write<'a>(
                   &self,
                   data: &mut ThickPtr<u8>,
                   fds: &mut ThickPtr<RawFd>,
               ) -> primitives::Result<()> {
                   #(#fields_write)*
                   Ok(())
               }

            }
        }
    };

    quote! {
        #item
        #impl_message
    }
}

fn gen_field(arg: &Arg) -> TokenStream {
    let Arg {
        name,
        typ,
        interface,
        summary,
        description,
        allow_null: _,
        enum_: _,
    } = arg;

    let name = mod_name(name);
    let docs = Docs::Local.summary(summary, description);

    let interface = interface.as_ref().map(|interface| {
        let mod_name = mod_name(interface);
        let typ_name = typ_name(interface);
        quote! {<#mod_name::#typ_name>}
    });

    let typ = match typ {
        Type::Int => quote! {    Int    },
        Type::Uint => quote! {   UInt   },
        Type::Fixed => quote! {  Fixed  },

        Type::Array => quote! {  Array <'data> },
        Type::String => quote! { String<'data> },

        Type::NewId if interface.is_none() => quote! {  NewIdDyn <'data> },

        Type::Object => quote! { Object #interface },
        Type::NewId => quote! {  NewId  #interface },

        Type::Fd => quote! {     Fd },
        Type::Destructor => unreachable!(),
    };

    quote! {
        #docs
        pub #name: #typ,
    }
}

fn generate_enum(enum_: &Enum) -> TokenStream {
    let Enum {
        name,
        since: _,
        description,
        entries,
        bitfield: _,
    } = enum_;

    let docs = Docs::Local.description(description);
    let name = typ_name(name);
    let entries = entries.iter().map(gen_entry);

    let impl_enum = impl_enum(enum_);

    quote! {
        #docs
        #[derive(Debug, Clone, Copy)]
        pub enum #name {
            #(#entries)*
        }

        #impl_enum
    }
}

fn impl_enum(enum_: &Enum) -> TokenStream {
    let name = typ_name(&enum_.name);
    let variants = enum_
        .entries
        .iter()
        .map(|entry| {
            let value = Literal::u32_unsuffixed(entry.value);
            let name = typ_name(&entry.name);
            quote! {
                #value => Some(Self::#name),
            }
        })
        .collect::<TokenStream>();

    quote! {
        impl Enum for #name {
            fn from_u32(int: u32) -> Option<Self> {
                match int {
                    #variants
                    _ => None,
                }
            }

            fn to_u32(&self) -> u32 {
                *self as u32
            }
        }
    }
}

fn gen_entry(entry: &Entry) -> TokenStream {
    let Entry {
        name,
        value,
        since: _,
        description,
        summary,
    } = entry;
    let name = typ_name(name);
    let docs = Docs::Local.summary(summary, description);
    let value = Literal::u32_unsuffixed(*value);
    quote! {
        #docs
        #name = #value,
    }
}

#[derive(Clone, Copy)]
enum Docs {
    Global,
    Local,
}

impl Docs {
    fn to_attr<T: ToTokens>(self, msg: T) -> TokenStream {
        match self {
            Docs::Global => {
                quote! { #![doc = #msg] }
            }
            Docs::Local => {
                quote! { #[doc = #msg] }
            }
        }
    }

    fn with_iter<'a>(self, iter: impl Iterator<Item = &'a str>) -> TokenStream {
        iter.filter(|str| !str.is_empty())
            .map(|desc| {
                desc.lines().map({
                    let mut buf = String::new();
                    move |str| {
                        buf.clear();
                        buf.reserve(str.len() + 1);

                        buf += " ";

                        const PATTERN: &[char] = &['[', ']'];
                        for next in str.split_inclusive(PATTERN) {
                            let mut segment = next.chars();
                            match segment.next_back() {
                                Some(char) => {
                                    if PATTERN.contains(&char) {
                                        buf += segment.as_str();
                                        buf += "\\";
                                        buf.write_char(char).unwrap();
                                    } else {
                                        buf += next;
                                    }
                                }
                                None => todo!(),
                            }
                        }

                        self.to_attr(&buf)
                    }
                })
            })
            .iter_flat_map(
                |iter| iter.next(),
                move |iter, acc| match acc {
                    None => {
                        *acc = Some(iter.next()?);
                        Some(self.to_attr(""))
                    }
                    Some(iter) => iter.next(),
                },
            )
            .collect()
    }

    fn description(self, description: &Option<(String, String)>) -> TokenStream {
        self.with_iter(
            description
                .as_ref()
                .map(|(a, b)| [a.as_str(), b.as_str()])
                .into_iter()
                .flatten(),
        )
    }

    fn summary(
        self,
        summary: &Option<String>,
        description: &Option<(String, String)>,
    ) -> TokenStream {
        self.with_iter(
            summary.as_deref().into_iter().chain(
                description
                    .as_ref()
                    .map(|(a, b)| [a.as_str(), b.as_str()])
                    .into_iter()
                    .flatten(),
            ),
        )
    }
}

fn mod_name(name: &str) -> syn::Ident {
    format_ident!("{name}")
}

fn typ_name(name: &str) -> syn::Ident {
    let name = wayland_scanner_lib::util::snake_to_camel(name);
    format_ident!(
        "{prefix}{name}",
        prefix = if name.chars().next().unwrap().is_numeric() {
            "_"
        } else {
            ""
        }
    )
}
