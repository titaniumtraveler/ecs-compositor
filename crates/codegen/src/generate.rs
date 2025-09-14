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
        #[allow(unused_variables,unused_mut,unused_imports, dead_code, non_camel_case_types, unused_unsafe)]
        #[allow(clippy::doc_lazy_continuation,clippy::identity_op, clippy::match_single_binding)]
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
        quote! {enumeration::#name}
    } else {
        quote! {uint}
    };

    let typ_name = typ_name(name);
    let mod_name = mod_name(name);

    let docs = Docs::Global.description(description);

    let iface_name = {
        let version = Literal::u32_unsuffixed(*version);

        quote! {
            use {
                super::super::{interfaces::*, proto::{self, *}},
                std::os::fd::RawFd,
            };

            pub enum #typ_name {}
            impl proto::Interface for #typ_name {
                const NAME:   &str = #name;
                const VERSION: u32 = #version;

                type Request = request::Opcodes;
                type Event   = event::Opcodes;

                type Error   = #error;
            }
        }
    };

    let requests = {
        let opcodes = gen_message_opcodes(requests);
        let requests = requests.iter().map(|msg| generate_message(msg, &typ_name));

        quote! {
            pub mod request {
                use super::*;
                #opcodes

                #(#requests)*
            }
        }
    };
    let events = {
        let opcodes = gen_message_opcodes(events);
        let events = events.iter().map(|msg| generate_message(msg, &typ_name));

        quote! {
            pub mod event {
                use super::*;
                #opcodes

                #(#events)*
            }
        }
    };
    let enumerations = {
        let enums = enums.iter().map(generate_enum);
        quote! {
            pub mod enumeration {
                use super::{*, proto::enumeration};
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
            #enumerations
        }
    }
}

fn gen_message_opcodes(messages: &[Message]) -> TokenStream {
    let entry = messages.iter().enumerate().map(|(i, msg)| {
        let name = self::typ_name(&msg.name);
        let i = Literal::u16_unsuffixed(i.try_into().expect("requests overflowing u16"));
        quote! { #name = #i, }
    });

    let from_u16 = messages.iter().enumerate().map(|(i, msg)| {
        let name = self::typ_name(&msg.name);
        let i = Literal::u16_unsuffixed(i.try_into().expect("requests overflowing u16"));
        quote! { #i => Ok(Self::#name), }
    });

    quote! {
        #[derive(Debug, Clone, Copy)]
        pub enum Opcodes {
            #(#entry)*
        }

        impl proto::Opcode for Opcodes {
            fn from_u16(i: u16) -> std::result::Result<Self, u16> {
                match i {
                    #(#from_u16)*
                    err => Err(err),
                }
            }

            fn to_u16(self) -> u16 {
                self as u16
            }
        }
    }
}

fn generate_message(message: &Message, iface_name: &syn::Ident) -> TokenStream {
    let Message {
        name,
        typ: _,
        since,
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
        let version = Literal::u32_unsuffixed(*since);

        let fd_count = Literal::usize_unsuffixed(
            args.iter()
                .filter(|arg| matches!(arg.typ, Type::Fd))
                .count(),
        );

        let fields_read = args.iter().map(|arg| {
            let name = mod_name(&arg.name);
            let typ = match arg.typ {
                Type::NewId if arg.interface.is_none() => quote! { new_id_dyn },

                Type::Int => quote! {    int    },
                Type::Uint => quote! {   uint   },
                Type::Fixed => quote! {  fixed  },
                Type::String => quote! { string },
                Type::Object => quote! { object },
                Type::NewId => quote! {  new_id  },
                Type::Array => quote! {  array  },
                Type::Fd => quote! {     fd     },
                Type::Destructor => unreachable!(),
            };
            quote! {
                #name: #typ::read(data, fds)?,
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
            impl<'data> Message<'data> for #name #lifetime {
                type Interface = #iface_name;
                const VERSION: u32 = #version;

                type Opcode = Opcodes;
                const OPCODE: Self::Opcode = Self::Opcode::#name;
                const OP: u16 = Self::OPCODE as u16;

                const FDS: usize = #fd_count;
            }

            impl<'data> Value<'data> for #name #lifetime {
                unsafe fn read(
                    data: &mut *const [u8],
                    fds: &mut *const [RawFd],
                ) -> primitives::Result<Self> {
                    unsafe {
                        Ok(Self {
                            #(#fields_read)*
                        })
                    }
                }

                fn len(&self) -> u32 {
                    0 #(#fields_write_len)*
                }

                unsafe fn write(
                    &self,
                    data: &mut *mut [u8],
                    fds: &mut *mut [RawFd],
                ) -> primitives::Result<()> {
                    unsafe {
                        #(#fields_write)*
                        Ok(())
                    }
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
        Type::Int => quote! {    int    },
        Type::Uint => quote! {   uint   },
        Type::Fixed => quote! {  fixed  },

        Type::Array => quote! {  array <'data> },
        Type::String => quote! { string<'data> },

        Type::NewId if interface.is_none() => quote! {  new_id_dyn <'data> },

        Type::Object => quote! { object #interface },
        Type::NewId => quote! {  new_id  #interface },

        Type::Fd => quote! {     fd },
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
    let variants = enum_.entries.iter().map(|entry| {
        let value = Literal::u32_unsuffixed(entry.value);
        let name = typ_name(&entry.name);
        quote! {
            #value => Some(Self::#name),
        }
    });
    let versions = enum_.entries.iter().map(|entry| {
        let name = typ_name(&entry.name);
        let version = Literal::u32_unsuffixed(entry.since as u32);
        quote! { Self::#name => #version, }
    });

    quote! {
        impl proto::enumeration for #name {
            fn from_u32(i: u32) -> Option<Self> {
                match i {
                    #(#variants)*
                    _ => None,
                }
            }

            fn to_u32(&self) -> u32 {
                *self as u32
            }

            fn since_version(&self) -> u32 {
                match self {
                    #(#versions)*
                }
            }
        }

        impl Value<'_> for #name {
                unsafe fn read(
                    data: &mut *const [u8],
                    fds: &mut *const [RawFd],
                ) -> primitives::Result<Self> {
                    todo!()
                }

                fn len(&self) -> u32 {
                    uint(self.to_u32()).len()
                }

                unsafe fn write(
                    &self,
                    data: &mut *mut [u8],
                    fds: &mut *mut [RawFd],
                ) -> primitives::Result<()> {
                    todo!()
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
    format_ident!(
        "{prefix}{name}",
        prefix = match () {
            _ if is_numeric(name) => "_",
            _ if is_keyword(name) => "_",
            _ => "",
        }
    )
}

fn is_numeric(str: &str) -> bool {
    str.chars().next().unwrap().is_numeric()
}

fn is_keyword(str: &str) -> bool {
    matches!(str, "move")
}
