use tracing_subscriber::{EnvFilter, Layer, layer::SubscriberExt, util::SubscriberInitExt};

pub mod custom_formatter;
pub mod dbg_u64;

pub fn setup_tracing() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(true)
                .pretty()
                .event_format(custom_formatter::CustomFormatter)
                .fmt_fields(custom_formatter::FormattedFields::new())
                .with_filter(EnvFilter::from_default_env()),
        )
        .init();
}
