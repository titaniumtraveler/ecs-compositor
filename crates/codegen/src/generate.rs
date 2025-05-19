use proc_macro2::{Literal, TokenStream};
use quote::{format_ident, quote};
use wayland_scanner_lib::protocol::{Arg, Entry, Enum, Interface, Message, Protocol, Type};

pub fn generate_protocol(protocol: &Protocol) -> TokenStream {
    let Protocol {
        name,
        description,
        interfaces,
        ..
    } = protocol;

    let desc = desc(&None, description);
    let name = mod_name(name);
    let interfaces = interfaces.iter().map(generate_interface);
    quote! {
        #desc
        pub mod #name {
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

    let mod_name = mod_name(name);
    let typ_name = typ_name(name);
    let desc = desc(&None, description);

    let requests = requests.iter().map(generate_message);
    let events = events.iter().map(generate_message);
    let enums = enums.iter().map(generate_enum);

    quote! {
        #desc
        pub mod #mod_name {
            pub enum #typ_name {}
            impl ::ecs_compositor_core::Interface for #typ_name {
                const NAME:   &str = #name;
                const VERSION: u32 = #version;

                type Error = u32;
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

fn generate_message(message: &Message) -> TokenStream {
    let Message {
        name,
        typ: _,
        since: _,
        description,
        args,
    } = message;

    let name = typ_name(name);
    let desc = desc(&None, description);
    let fields = args.iter().map(gen_field);

    quote! {
        #desc
        pub struct #name {
            #(#fields)*
        }
    }
}

fn gen_field(arg: &Arg) -> TokenStream {
    let Arg {
        name,
        typ,
        interface: _,
        summary,
        description,
        allow_null: _,
        enum_: _,
    } = arg;

    let name = mod_name(name);
    let desc = desc(summary, description);

    let typ = match typ {
        Type::Int => quote! { i32 },
        Type::Uint => quote! { u32 },
        Type::Fixed => quote! { f64 },
        Type::String => quote! { (std::ptr::NonNull<u32>, u32) },
        Type::Object => quote! { std::num::NonZero<u32> },
        Type::NewId => quote! { std::num::NonZero<u32> },
        Type::Array => quote! { (std::ptr::NonNull<u32>, u32) },
        Type::Fd => quote! { std::os::fd::OwnedFd },
        Type::Destructor => unreachable!(),
    };

    quote! {
        #desc
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

    let desc = desc(&None, description);
    let name = typ_name(name);
    let entries = entries.iter().map(gen_entry);

    quote! {
        #desc
        pub enum #name {
            #(#entries)*
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
    let desc = desc(summary, description);
    let value = Literal::u32_unsuffixed(*value);
    quote! {
        #desc
        #name = #value,
    }
}

fn desc(summary: &Option<String>, description: &Option<(String, String)>) -> TokenStream {
    let summary = summary.as_deref().unwrap_or("");
    let (desc_short, desc) = if let Some((desc_short, desc)) = &description {
        (desc_short.as_ref(), desc.as_ref())
    } else {
        ("", "")
    };

    let desc = format!("{summary}\n\n{desc_short}\n\n{desc}")
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned();

    quote! {
        #[doc = #desc]
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
