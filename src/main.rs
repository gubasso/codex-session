//! `codex-session` CLI binary.
//!
//! Architecture follows `/home/gu/Projects/docs-n-notes/tech/languages/rust/cli-spec/`.
//! `main` does only: parse argv → load config → init logging → build
//! `AppContext` → dispatch → exit code. Pre-parse argv helpers live in
//! `cli::argv`; clap-error handling in `cli::exit`; verb dispatch in
//! `commands::dispatch`.

#![allow(clippy::redundant_pub_crate)]

pub(crate) mod adapters;
pub(crate) mod cli;
pub(crate) mod commands;
pub(crate) mod config;
pub(crate) mod context;
pub(crate) mod domain;
pub(crate) mod error;
pub(crate) mod logging;
pub(crate) mod services;
pub(crate) mod ui;

use clap::Parser as _;
use std::ffi::OsString;
use std::process::ExitCode;
use std::sync::Arc;

#[cfg(not(unix))]
compile_error!("codex-session is Unix-only");

fn main() -> ExitCode {
    let argv = cli::argv::normalize_argv(std::env::args_os().skip(1).collect());
    if cli::argv::legacy_self_invocation(&argv) {
        let err = clap::Error::raw(
            clap::error::ErrorKind::InvalidSubcommand,
            "unrecognized subcommand 'self'",
        );
        return cli::exit::handle_clap_error(&err, &argv);
    }
    let cli = match cli::Cli::try_parse_from(
        std::iter::once(OsString::from("codex-session")).chain(argv.iter().cloned()),
    ) {
        Ok(cli) => cli,
        Err(err) => return cli::exit::handle_clap_error(&err, &argv),
    };
    let overrides = config::CliOverrides::from_global(&cli.global);
    let config = match config::Config::load(&overrides) {
        Ok(config) => Arc::new(config),
        Err(err) => {
            return print_and_exit(&error::AppError::from_config_error(err), cli.global.silent);
        }
    };
    let log_options = logging::options_from_config(&config, &cli.global);
    let _log = match logging::init(&log_options) {
        Ok(handle) => handle,
        Err(err) => {
            return print_and_exit(
                &error::AppError::Other(anyhow::anyhow!("failed to install tracing: {err}")),
                cli.global.silent,
            );
        }
    };
    let ctx = context::AppContext::new(Arc::clone(&config), cli.global.clone());
    match commands::dispatch::run(&ctx, cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => print_and_exit(&err, ctx.global.silent),
    }
}

pub(crate) fn print_and_exit(err: &error::AppError, silent: bool) -> ExitCode {
    error::log_error(err);
    if !silent {
        let _ = error::render_error(err);
    }
    ExitCode::from(err.exit_code())
}
