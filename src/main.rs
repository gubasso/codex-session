//! Binary entry point.

pub mod adapters;
pub mod cli;
pub mod commands;
pub mod context;
pub mod domain;
pub mod error;
pub mod logging;
pub mod services;
pub mod ui;

use std::ffi::OsString;
use std::process::ExitCode;

#[cfg(not(unix))]
compile_error!("codex-session is Unix-only");

fn main() -> ExitCode {
    let argv: Vec<OsString> = std::env::args_os().skip(1).collect();
    let ctx = match context::AppContext::new() {
        Ok(ctx) => ctx,
        Err(err) => return print_and_exit(&err),
    };

    match services::dispatch::run(&ctx, &argv) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => print_and_exit(&err),
    }
}

fn print_and_exit(error: &error::AppError) -> ExitCode {
    eprintln!("{error}");
    ExitCode::from(error.exit_code())
}
