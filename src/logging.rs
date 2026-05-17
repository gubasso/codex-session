//! tracing-subscriber installation. Called once from `main`.

use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

/// Install the global tracing subscriber.
pub(crate) fn init() -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_writer(std::io::stderr).with_target(false))
        .try_init()
        .map_err(|err| anyhow::anyhow!("install tracing subscriber: {err}"))?;
    Ok(())
}
