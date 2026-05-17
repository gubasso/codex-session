//! tracing-subscriber installation. Called once from `main`.

use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

/// Install the global tracing subscriber.
#[allow(dead_code)]
fn init(verbosity: u8) -> anyhow::Result<()> {
    let default_directive = match verbosity {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_directive));

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_writer(std::io::stderr).with_target(false))
        .try_init()
        .map_err(|err| anyhow::anyhow!("install tracing subscriber: {err}"))?;
    Ok(())
}
