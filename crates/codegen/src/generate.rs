use crate::generate::flat_map_fn::IteratorExt;
use proc_macro2::{Literal, TokenStream};
use quote::{format_ident, quote};
use wayland_scanner_lib::protocol;

mod flat_map_fn;

pub fn generate_protocol(protocol: &protocol::Protocol) -> TokenStream {
    let protocol::Protocol {
        name,
        description,
        interfaces,
        ..
    } = protocol;

    let desc = desc(desc_as_iter(description));
    let name = mod_name(name);
    let interfaces = interfaces.iter().map(generate_interface);
    quote! {
        #[allow(clippy::doc_lazy_continuation)]

        #desc
        pub mod #name {
            #(#interfaces)*
        }
    }
}

fn generate_interface(interface: &protocol::Interface) -> TokenStream {
    let protocol::Interface {
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

    let mod_name = mod_name(name);
    let typ_name = typ_name(name);
    let desc = desc(desc_as_iter(description));

    let requests = requests.iter().map(generate_message);
    let events = events.iter().map(generate_message);
    let enums = enums.iter().map(generate_enum);

    quote! {
        #desc
        pub mod #mod_name {
            pub enum #typ_name {}
            impl ecs_compositor_core::Interface for #typ_name {
                const NAME:   &str = #name;
                const VERSION: u32 = #version;

                type Error = #error;
            }

            pub mod requests {
                #(#requests)*
            }

            pub mod events {
                #(#events)*
            }

            pub mod enums {
                #(#enums)*
            }
        }
    }
}

fn generate_message(message: &protocol::Message) -> TokenStream {
    let protocol::Message {
        name,
        typ: _,
        since: _,
        description,
        args,
    } = message;

    let name = typ_name(name);
    let desc = desc(desc_as_iter(description));
    let fields = args.iter().map(gen_field);

    let lifetime = if message
        .args
        .iter()
        .any(|arg| matches!(arg.typ, protocol::Type::Array | protocol::Type::String))
    {
        quote! {'data}
    } else {
        TokenStream::default()
    };

    quote! {
        #desc
        pub struct #name<#lifetime> {
            #(#fields)*
        }
    }
}

fn gen_field(arg: &protocol::Arg) -> TokenStream {
    let protocol::Arg {
        name,
        typ,
        interface: _,
        summary,
        description,
        allow_null: _,
        enum_: _,
    } = arg;

    let name = mod_name(name);
    let desc = desc(
        summary
            .as_deref()
            .into_iter()
            .chain(desc_as_iter(description)),
    );

    let typ = match typ {
        protocol::Type::Int => quote! {    ecs_compositor_core::Int    },
        protocol::Type::Uint => quote! {   ecs_compositor_core::UInt   },
        protocol::Type::Fixed => quote! {  ecs_compositor_core::Fixed  },

        protocol::Type::Array => quote! {  ecs_compositor_core::Array<'data>  },
        protocol::Type::String => quote! { ecs_compositor_core::String<'data> },

        protocol::Type::Object => quote! { ecs_compositor_core::Object },
        protocol::Type::NewId => quote! {  ecs_compositor_core::NewId  },

        protocol::Type::Fd => quote! {     ecs_compositor_core::Fd },
        protocol::Type::Destructor => unreachable!(),
    };

    quote! {
        #desc
        pub #name: #typ,
    }
}

fn generate_enum(enum_: &protocol::Enum) -> TokenStream {
    let protocol::Enum {
        name,
        since: _,
        description,
        entries,
        bitfield: _,
    } = enum_;

    let desc = desc(desc_as_iter(description));
    let name = typ_name(name);
    let entries = entries.iter().map(gen_entry);

    let impl_enum = impl_enum(enum_);

    quote! {
        #desc
        #[derive(Debug, Clone, Copy)]
        pub enum #name {
            #(#entries)*
        }

        #impl_enum
    }
}

fn impl_enum(enum_: &protocol::Enum) -> TokenStream {
    let name = typ_name(&enum_.name);
    let variants = enum_
        .entries
        .iter()
        .map(|entry| {
            let value = entry.value;
            let name = typ_name(&entry.name);
            quote! {
                #value => Some(Self::#name),
            }
        })
        .collect::<TokenStream>();

    quote! {
        impl ecs_compositor_core::Enum for #name {
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

fn gen_entry(entry: &protocol::Entry) -> TokenStream {
    let protocol::Entry {
        name,
        value,
        since: _,
        description,
        summary,
    } = entry;
    let name = typ_name(name);
    let desc = desc(
        summary
            .as_deref()
            .into_iter()
            .chain(desc_as_iter(description)),
    );
    let value = Literal::u32_unsuffixed(*value);
    quote! {
        #desc
        #name = #value,
    }
}

fn desc_as_iter(description: &Option<(String, String)>) -> impl Iterator<Item = &str> {
    description
        .as_ref()
        .map(|(a, b)| [a.as_str(), b.as_str()])
        .into_iter()
        .flatten()
}

fn desc<'a>(iter: impl Iterator<Item = &'a str>) -> TokenStream {
    iter.filter(|str| !str.is_empty())
        .map(|desc| {
            desc.lines().map({
                let mut buf = String::new();
                move |str| {
                    buf.clear();
                    buf.extend([" ", str]);
                    quote! { #[doc = #buf] }
                }
            })
        })
        .iter_flat_map(
            |iter| iter.next(),
            move |iter, acc| match acc {
                None => {
                    *acc = Some(iter.next()?);
                    Some(quote! { #[doc = ""]})
                }
                Some(iter) => iter.next(),
            },
        )
        .collect()
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
