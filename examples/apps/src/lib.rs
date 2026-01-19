use tracing_subscriber::{EnvFilter, Layer, layer::SubscriberExt, util::SubscriberInitExt};

pub mod bind;
mod custom_formatter;
pub mod protocols;

pub fn setup_tracing() {
    tracing_subscriber::registry()
        .with(console_subscriber::spawn())
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(true)
                .pretty()
                // .json()
                // .event_format(crate::custom_formatter::CustomFormatter)
                // .with_writer(std::fs::File::create("log.json").unwrap())
                .with_filter(EnvFilter::from_default_env()),
        )
        .init();
}
