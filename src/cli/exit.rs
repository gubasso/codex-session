//! Wrapper-side handling of clap's early exits.
//!
//! What this is: `handle_clap_error` (called from `main` on every
//! `Cli::try_parse_from` failure) plus the curated `HELP_TEXT` it serves
//! for `--help`. Keeping these here keeps `src/main.rs` under its 120-LOC
//! budget while letting both the `help` subcommand and `--help` route
//! through the same source of truth.
//!
//! What this is not: dispatch (that lives in `commands::dispatch`).

use std::ffi::OsString;
use std::io::Write as _;
use std::process::ExitCode;

use crate::{config, error, logging};

/// Curated wrapper help text shared by `help` and `--help` / `-h`.
pub(crate) const HELP_TEXT: &str = include_str!("../ui/help.txt");

/// Map a `clap::Error` into the wrapper's exit-code contract.
///
/// `DisplayHelp` prints `HELP_TEXT` (so `--help` and `help` are pixel
/// equivalent). `DisplayVersion` defers to clap. Every other parse error
/// is routed through `AppError::Usage` (exit 64). A bad config encountered
/// on the error fallback path is surfaced as a `ConfigError` (exit 78)
/// rather than being silently demoted.
pub(crate) fn handle_clap_error(err: &clap::Error, argv: &[OsString]) -> ExitCode {
    use clap::error::ErrorKind::{DisplayHelp, DisplayVersion};

    match err.kind() {
        DisplayHelp => {
            let mut stdout = std::io::stdout().lock();
            let _ = stdout.write_all(HELP_TEXT.as_bytes());
            if !HELP_TEXT.ends_with('\n') {
                let _ = stdout.write_all(b"\n");
            }
            ExitCode::SUCCESS
        }
        DisplayVersion => {
            let _ = err.print();
            ExitCode::SUCCESS
        }
        _ => map_parse_failure(err, argv),
    }
}

fn map_parse_failure(err: &clap::Error, argv: &[OsString]) -> ExitCode {
    let global = super::argv::scan_global_args(argv);
    let overrides = config::CliOverrides::from_global(&global);
    match config::Config::load(&overrides) {
        Ok(config) => {
            let mirror_stderr = config.log.mirror_stderr || config.log.verbose > 0;
            let log_file = config
                .log
                .file
                .clone()
                .unwrap_or_else(|| config.paths.state_dir.join("codex-session.log"));
            if logging::init(config.log.verbose, log_file.as_std_path(), mirror_stderr).is_ok() {
                return crate::print_and_exit(&error::AppError::Usage(err.render().to_string()));
            }
            let _ = std::io::stderr()
                .lock()
                .write_all(err.render().to_string().as_bytes());
            ExitCode::from(error::AppError::Usage(String::new()).exit_code())
        }
        // A broken config is more severe than a clap usage error. Surface
        // the config error (exit 78) instead of hiding it behind the clap
        // fallback (exit 64).
        Err(config_err) => crate::print_and_exit(&error::AppError::from_config_error(config_err)),
    }
}
