//! Wrapper-side handling of clap's early exits.
//!
//! What this is: `handle_clap_error` (called from `main` on every
//! `Cli::try_parse_from` failure) plus the curated `HELP_TEXT` it serves
//! for `--help`. Keeping these here keeps `src/main.rs` under its 120-LOC
//! budget while letting both the `help` subcommand and `--help` route
//! through the same source of truth.
//!
//! What this is not: dispatch (that lives in `commands::dispatch`).

use std::{ffi::OsString, process::ExitCode};

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
            let _ = crate::ui::Ui::new().write_help(HELP_TEXT);
            ExitCode::SUCCESS
        }
        DisplayVersion => unreachable!("clap version flag is disabled on the root parser"),
        _ => map_parse_failure(err, argv),
    }
}

fn map_parse_failure(err: &clap::Error, argv: &[OsString]) -> ExitCode {
    let global = super::argv::scan_global_args(argv);
    let overrides = config::CliOverrides::from_global(&global);
    match config::Config::load(&overrides) {
        Ok(config) => {
            let log_options = logging::options_from_config(&config, &global);
            if let Ok(_log) = logging::init(&log_options) {
                return crate::print_and_exit(
                    &error::AppError::Usage(err.render().to_string()),
                    global.silent,
                );
            }
            if !global.silent {
                let _ = error::render_error(&error::AppError::Usage(err.render().to_string()));
            }
            ExitCode::from(error::AppError::Usage(String::new()).exit_code())
        }
        // A broken config is more severe than a clap usage error. Surface
        // the config error (exit 78) instead of hiding it behind the clap
        // fallback (exit 64).
        Err(config_err) => crate::print_and_exit(
            &error::AppError::from_config_error(config_err),
            global.silent,
        ),
    }
}
