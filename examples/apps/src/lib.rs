use tracing_subscriber::{EnvFilter, Layer, layer::SubscriberExt, util::SubscriberInitExt};

pub fn setup_tracing() {
    tracing_subscriber::registry()
        .with(console_subscriber::spawn())
        .with(tracing_subscriber::fmt::layer().with_filter(EnvFilter::from_default_env()))
        .init();
}

pub mod protocols {
    mod interfaces {
        pub use super::wayland::*;
    }

    pub use ecs_compositor_core as proto;

    include!(concat!(env!("OUT_DIR"), "/wayland-core.rs"));
}
