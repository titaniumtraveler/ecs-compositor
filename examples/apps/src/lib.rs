use tracing_subscriber::{EnvFilter, Layer, layer::SubscriberExt, util::SubscriberInitExt};

mod custom_formatter;

pub fn setup_tracing() {
    tracing_subscriber::registry()
        .with(console_subscriber::spawn())
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(true)
                .pretty()
                // .json()
                .event_format(crate::custom_formatter::CustomFormatter)
                // .with_writer(std::fs::File::create("log.json").unwrap())
                .with_filter(EnvFilter::from_default_env()),
        )
        .init();
}

#[macro_export]
macro_rules! protocols {
    () => {
        pub mod protocols {
            mod interfaces {
                pub use super::{wayland::*, wlr::wlr_gamma_control_unstable_v1::*};
            }

            pub use ecs_compositor_core as proto;

            include!(concat!(env!("OUT_DIR"), "/wayland.rs"));

            pub mod wlr {
                use super::*;

                include!(concat!(
                    env!("OUT_DIR"),
                    "/wlr/wlr-gamma-control-unstable-v1.rs"
                ));
            }
        }
    };
}
