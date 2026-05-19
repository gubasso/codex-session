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
        Err(err) => return print_and_exit(&error::AppError::from_config_error(err)),
    };
    let mirror_stderr = config.log.mirror_stderr || config.log.verbose > 0;
    let log_file = config
        .log
        .file
        .clone()
        .unwrap_or_else(|| config.paths.state_dir.join("codex-session.log"));
    let _log = match logging::init(config.log.verbose, log_file.as_std_path(), mirror_stderr) {
        Ok(handle) => handle,
        Err(err) => {
            return print_and_exit(&error::AppError::Other(anyhow::anyhow!(
                "failed to install tracing: {err}"
            )));
        }
    };
    let ctx = context::AppContext::new(Arc::clone(&config), cli.global.clone());
    match commands::dispatch::run(&ctx, cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => print_and_exit(&err),
    }
}

pub(crate) fn print_and_exit(err: &error::AppError) -> ExitCode {
    error::log_error(err);
    let mut stderr = std::io::stderr().lock();
    let _ = error::render(&mut stderr, err);
    ExitCode::from(err.exit_code())
}
