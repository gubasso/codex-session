//! `codex-session` CLI binary.
//!
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
    let overrides = config::CliOverrides::from_global(&cli.global);
    let config = match config::Config::load(&overrides) {
        Ok(config) => Arc::new(config),
        Err(err) => {
            return error::print_and_exit(&error::AppError::Config(err), &cli.global);
        }
    };
    let home_dir = match config::resolve_home_dir() {
        Ok(home_dir) => home_dir,
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
    let ctx = context::AppContext::new(Arc::clone(&config), cli.global.clone(), home_dir);
    let state_root = &ctx.config.paths.state_dir;
    let runtime_root = ctx.config.paths.runtime_dir.as_deref();
    crate::services::session::cleanup::prune_legacy_pid_dirs(state_root, runtime_root);

    let accounts_root = state_root.join("accounts");
    crate::services::session::cleanup::prune_stale_sessions_all_accounts(
        &accounts_root,
        std::time::Duration::from_secs(7 * 24 * 3600),
    );
    match commands::dispatch::run(&ctx, cli) {
        Ok(code) => ExitCode::from(code),
        Err(err) => error::print_and_exit(&err, &ctx.global),
    }
}
