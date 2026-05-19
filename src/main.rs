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
        return cli::exit::handle_clap_error(err, &argv);
    }
    let argv_iter = std::iter::once(OsString::from("codex-session")).chain(argv.iter().cloned());
    let mut matches = match <cli::Cli as clap::CommandFactory>::command()
        .color(ui::color::stdout_color_choice())
        .try_get_matches_from(argv_iter)
    {
        Ok(matches) => matches,
        Err(err) => return cli::exit::handle_clap_error(err, &argv),
    };
    let cli = match <cli::Cli as clap::FromArgMatches>::from_arg_matches_mut(&mut matches) {
        Ok(cli) => cli,
        Err(err) => return cli::exit::handle_clap_error(err, &argv),
    };
    // Bare `codex-session` (no subcommand, no `--version`) renders the
    // same long help as `codex-session --help` / `codex-session help`.
    // Per the Phase 11 Tier 1 contract, all three entry points must be
    // functionally equivalent — including not depending on config /
    // logging init. Short-circuit here so an unwritable log dir cannot
    // poison the bare help path.
    if cli.command.is_none() && !cli.global.version {
        use clap::CommandFactory;
        let mut cmd = cli::Cli::command().color(ui::color::stdout_color_choice());
        let _ = cmd.print_long_help();
        return ExitCode::SUCCESS;
    }
    let overrides = config::CliOverrides::from_global(&cli.global);
    let config = match config::Config::load(&overrides) {
        Ok(config) => Arc::new(config),
        Err(err) => {
            return error::print_and_exit(&error::AppError::Config(err), &cli.global);
        }
    };
    let log_options = logging::options_from_config(&config, &cli.global);
    let _log = match logging::init(&log_options) {
        Ok(handle) => handle,
        Err(err) => {
            return error::print_and_exit(
                &error::AppError::Other(anyhow::anyhow!("failed to install tracing: {err}")),
                &cli.global,
            );
        }
    };
    let ctx = context::AppContext::new(Arc::clone(&config), cli.global.clone());
    match commands::dispatch::run(&ctx, cli) {
        Ok(code) => ExitCode::from(code),
        Err(err) => error::print_and_exit(&err, &ctx.global),
    }
}
