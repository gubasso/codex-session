//! Wrapper-side handling of clap's early exits.
//!
//! What this is: `handle_clap_error` plus the curated `HELP_TEXT` served for
//! `--help`.
//! What this is not: command dispatch; that lives in `commands::dispatch`.

use std::{ffi::OsString, process::ExitCode};

/// Curated wrapper help text shared by `help` and `--help` / `-h`.
pub(crate) const HELP_TEXT: &str = include_str!("../ui/help.txt");

/// Map a `clap::Error` into the wrapper's exit-code contract.
///
/// `DisplayHelp` prints `HELP_TEXT` (so `--help` and `help` are pixel
/// equivalent). `DisplayVersion` defers to clap. Every other parse error
/// is routed through `AppError::Usage` (exit 64).
pub(crate) fn handle_clap_error(err: clap::Error, _argv: &[OsString]) -> ExitCode {
    use clap::error::ErrorKind::{DisplayHelp, DisplayVersion};

    match err.kind() {
        DisplayHelp => {
            let _ = crate::ui::Ui::new().write_help(HELP_TEXT);
            ExitCode::SUCCESS
        }
        DisplayVersion => unreachable!("clap version flag is disabled on the root parser"),
        _ => crate::error::print_and_exit(
            &crate::error::AppError::Usage(err),
            &crate::cli::GlobalArgs::default(),
        ),
    }
}
