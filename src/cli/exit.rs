//! Wrapper-side handling of clap's early exits.
//!
//! What this is: `handle_clap_error` mapping clap parse errors onto the
//! wrapper's exit-code contract.
//! What this is not: command dispatch; that lives in `commands::dispatch`.

use std::{ffi::OsString, process::ExitCode};

/// Map a `clap::Error` into the wrapper's exit-code contract.
///
/// `DisplayHelp` is printed by clap itself (we do not intercept).
/// `DisplayVersion` is unreachable on the root parser because
/// `disable_version_flag = true` in `Cli`. Every other parse error is
/// routed through `AppError::Usage` (exit 64).
pub(crate) fn handle_clap_error(err: clap::Error, _argv: &[OsString]) -> ExitCode {
    use clap::error::ErrorKind::{DisplayHelp, DisplayVersion};

    match err.kind() {
        DisplayHelp => {
            let _ = err.print();
            ExitCode::SUCCESS
        }
        DisplayVersion => unreachable!("clap version flag is disabled on the root parser"),
        _ => crate::error::print_and_exit(
            &crate::error::AppError::Usage(err),
            &crate::cli::GlobalArgs::default(),
        ),
    }
}
