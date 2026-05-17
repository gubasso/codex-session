//! `codex-session` CLI binary.
//!
//! Architecture follows `/home/gu/Projects/docs-n-notes/tech/languages/rust/cli-spec/`:
//! - `cli`      — clap parse-shape for the `self` subtree.
//! - `commands` — handlers, one `run()` per verb.
//! - `services` — reusable orchestration (`merge`).
//! - `adapters` — outside-world I/O (`fs`, `process`).
//! - `domain`   — pure types (`paths`, `version`).
//! - `error`    — `AppError` + exit-code mapping.
//! - `logging`  — `tracing-subscriber` install, called once from `main`.
//! - `ui`       — the only place that writes to stdout.
#![allow(clippy::redundant_pub_crate)]

pub(crate) mod adapters;
pub(crate) mod cli;
pub(crate) mod commands;
pub(crate) mod context;
pub(crate) mod domain;
pub(crate) mod error;
pub(crate) mod logging;
pub(crate) mod services;
pub(crate) mod ui;

use std::ffi::OsString;
use std::process::ExitCode;

#[cfg(not(unix))]
compile_error!("codex-session is Unix-only");

fn main() -> ExitCode {
    if let Err(err) = logging::init() {
        eprintln!("codex-session: failed to install tracing: {err}");
        return ExitCode::from(70);
    }

    let argv: Vec<OsString> = std::env::args_os().skip(1).collect();
    let ctx = match context::AppContext::new() {
        Ok(c) => c,
        Err(e) => return print_and_exit(&e),
    };

    let result = match argv.first().and_then(|s| s.to_str()) {
        Some("self") => dispatch_self(&ctx, &argv[1..]),
        _ => commands::pass_through::run(&ctx, &argv),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => print_and_exit(&e),
    }
}

fn dispatch_self(ctx: &context::AppContext, rest: &[OsString]) -> Result<(), error::AppError> {
    use clap::Parser as _;
    use cli::self_cmd::{SelfArgs, SelfCommand};

    #[derive(clap::Parser)]
    #[command(
        name = "codex-session self",
        disable_help_flag = false,
        disable_help_subcommand = true
    )]
    struct SelfCli {
        #[command(flatten)]
        args: SelfArgs,
    }

    let mut argv = vec![OsString::from("codex-session self")];
    argv.extend(rest.iter().cloned());

    let parsed = match SelfCli::try_parse_from(&argv) {
        Ok(p) => p,
        Err(e) => {
            if e.kind() == clap::error::ErrorKind::InvalidSubcommand
                || e.kind() == clap::error::ErrorKind::UnknownArgument
            {
                let verb = rest
                    .first()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                return Err(error::AppError::UnknownSelfVerb(verb));
            }
            e.exit();
        }
    };

    match parsed
        .args
        .command
        .unwrap_or(SelfCommand::Help(crate::cli::self_help::SelfHelpArgs))
    {
        SelfCommand::Help(a) => commands::self_help::run(ctx, a),
        SelfCommand::Version(a) => commands::self_version::run(ctx, a),
        SelfCommand::ConfigStatus(a) => commands::self_config_status::run(ctx, a),
        SelfCommand::ConfigMerge(a) => commands::self_config_merge::run(ctx, a),
        SelfCommand::ShowLocal(a) => commands::self_show_local::run(ctx, a),
    }
}

fn print_and_exit(e: &error::AppError) -> ExitCode {
    eprintln!("{e}");
    if !matches!(
        e,
        error::AppError::CodexNotFound
            | error::AppError::BaseMissing(_)
            | error::AppError::UnknownSelfVerb(_)
    ) {
        let mut src: Option<&dyn std::error::Error> = std::error::Error::source(e);
        while let Some(cause) = src {
            eprintln!("  caused by: {cause}");
            src = cause.source();
        }
    }
    ExitCode::from(e.exit_code())
}
