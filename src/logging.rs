//! Logging bootstrap.
//!
//! What this is: resolution of effective log options plus
//! `tracing-subscriber` installation.
//! What this is not: application error rendering or CLI parsing.

use std::io::IsTerminal as _;

use camino::Utf8Path;
use tracing_appender::{non_blocking::WorkerGuard, rolling};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

/// Logging initialization state kept alive until shutdown.
#[must_use = "WorkerGuard must be kept alive for log flushing"]
pub(crate) struct LogInit {
    /// Background appender worker guard.
    pub(crate) _file_guard: WorkerGuard,
}

/// Resolved logging options.
pub(crate) struct LogOptions<'a> {
    /// Effective verbosity after config + CLI precedence is applied.
    pub(crate) verbose: u8,
    /// Directory where rotated log files live.
    pub(crate) log_dir: &'a Utf8Path,
    /// Optional stderr mirror.
    pub(crate) mirror: StderrMirror,
}

/// Stderr log-mirror mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StderrMirror {
    Off,
    Pretty,
    Json,
}

impl StderrMirror {
    /// Resolve the stderr mirror mode from CLI flags and effective verbosity.
    pub(crate) fn from_cli(
        quiet: bool,
        silent: bool,
        log_stderr: bool,
        format: Option<crate::config::LogFormat>,
        verbose: u8,
    ) -> Self {
        if silent || quiet {
            return Self::Off;
        }

        if !(log_stderr || verbose > 0) {
            return Self::Off;
        }

        match format.unwrap_or_else(|| {
            if std::io::stderr().is_terminal() {
                crate::config::LogFormat::Pretty
            } else {
                crate::config::LogFormat::Json
            }
        }) {
            crate::config::LogFormat::Pretty => Self::Pretty,
            crate::config::LogFormat::Json => Self::Json,
        }
    }
}

/// Resolve the directory that owns the rotating log files.
pub(crate) fn log_dir_from_config(config: &crate::config::Config) -> &Utf8Path {
    config
        .log
        .file
        .as_deref()
        .unwrap_or_else(|| config.paths.state_dir.as_ref())
}

/// Build logging options from config + global CLI flags.
pub(crate) fn options_from_config<'a>(
    config: &'a crate::config::Config,
    global: &crate::cli::GlobalArgs,
) -> LogOptions<'a> {
    let verbose = global.verbose.max(config.log.verbose);
    let mirror = StderrMirror::from_cli(
        global.quiet,
        global.silent,
        global.log_stderr || config.log.mirror_stderr,
        global.log_format.or(config.log.stderr_format),
        verbose,
    );

    LogOptions {
        verbose,
        log_dir: log_dir_from_config(config),
        mirror,
    }
}

/// Install the global tracing subscriber.
pub(crate) fn init(opts: &LogOptions<'_>) -> anyhow::Result<LogInit> {
    let default_directive = match opts.verbose {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_directive));

    std::fs::create_dir_all(opts.log_dir.as_std_path())
        .map_err(|err| anyhow::anyhow!("create log directory {}: {err}", opts.log_dir.as_str()))?;

    let appender = rolling::Builder::new()
        .rotation(rolling::Rotation::DAILY)
        .filename_prefix("codex-session.log")
        .build(opts.log_dir.as_std_path())
        .map_err(|err| anyhow::anyhow!("build rolling log appender: {err}"))?;
    let (file_writer, file_guard) = tracing_appender::non_blocking(appender);

    let registry = tracing_subscriber::registry().with(filter).with(
        fmt::layer()
            .with_writer(file_writer)
            .with_ansi(false)
            .with_target(true)
            .json(),
    );

    match opts.mirror {
        StderrMirror::Off => registry
            .try_init()
            .map_err(|err| anyhow::anyhow!("install tracing subscriber: {err}"))?,
        StderrMirror::Pretty => registry
            .with(
                fmt::layer()
                    .with_writer(std::io::stderr)
                    .with_target(false)
                    .with_ansi(crate::ui::color::stderr_color()),
            )
            .try_init()
            .map_err(|err| anyhow::anyhow!("install tracing subscriber: {err}"))?,
        StderrMirror::Json => registry
            .with(
                fmt::layer()
                    .with_writer(std::io::stderr)
                    .with_target(true)
                    .with_ansi(false)
                    .json(),
            )
            .try_init()
            .map_err(|err| anyhow::anyhow!("install tracing subscriber: {err}"))?,
    }

    Ok(LogInit {
        _file_guard: file_guard,
    })
}
