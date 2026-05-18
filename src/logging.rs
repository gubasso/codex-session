//! tracing-subscriber installation. Called once from `main`.

use std::fs::OpenOptions;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

/// Logging initialization state kept alive until shutdown.
pub(crate) struct LogInit {
    /// Shared log file handle kept alive for the process lifetime.
    pub(crate) _file: Arc<Mutex<std::fs::File>>,
}

/// Install the global tracing subscriber.
pub(crate) fn init(verbosity: u8, log_file: &Path, mirror_stderr: bool) -> anyhow::Result<LogInit> {
    let default_directive = match verbosity {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_directive));

    let dir = log_file
        .parent()
        .ok_or_else(|| anyhow::anyhow!("log file has no parent: {}", log_file.display()))?;
    std::fs::create_dir_all(dir)
        .map_err(|err| anyhow::anyhow!("create log directory {}: {err}", dir.display()))?;
    let file = Arc::new(Mutex::new(
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_file)
            .map_err(|err| anyhow::anyhow!("open log file {}: {err}", log_file.display()))?,
    ));

    let registry = tracing_subscriber::registry().with(filter).with(
        fmt::layer()
            .with_writer(JsonLogWriter(file.clone()))
            .with_ansi(false)
            .with_target(true)
            .json(),
    );

    if mirror_stderr {
        // Per `rust/cli-spec/04-logging.md` §"Variant: human-readable text
        // format": the file sink is JSON for LLM-friendliness, but the
        // optional terminal mirror keeps the default pretty formatter and
        // only disables colors when NO_COLOR is set. The two layers
        // intentionally use different formats.
        registry
            .with(
                fmt::layer()
                    .with_writer(std::io::stderr)
                    .with_target(false)
                    .with_ansi(std::env::var_os("NO_COLOR").is_none()),
            )
            .try_init()
            .map_err(|err| anyhow::anyhow!("install tracing subscriber: {err}"))?;
    } else {
        registry
            .try_init()
            .map_err(|err| anyhow::anyhow!("install tracing subscriber: {err}"))?;
    }

    Ok(LogInit { _file: file })
}

#[derive(Clone)]
struct JsonLogWriter(Arc<Mutex<std::fs::File>>);

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for JsonLogWriter {
    type Writer = LockedFileWriter<'a>;

    fn make_writer(&'a self) -> Self::Writer {
        LockedFileWriter {
            guard: match self.0.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            },
        }
    }
}

struct LockedFileWriter<'a> {
    guard: MutexGuard<'a, std::fs::File>,
}

impl std::io::Write for LockedFileWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.guard.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.guard.flush()
    }
}
