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
    use std::ffi::OsStr;

    // Bash semantics (`${1:-help}`): a missing OR empty first token defaults
    // to "help". Non-UTF-8 tokens are never equal to any known verb and must
    // fall through to the unknown-verb arm (rendered with lossy display).
    let token = rest
        .first()
        .map_or_else(|| OsStr::new(""), OsString::as_os_str);
    let token = if token.is_empty() {
        OsStr::new("help")
    } else {
        token
    };

    if token == OsStr::new("help") {
        commands::self_help::run(ctx, cli::self_help::SelfHelpArgs)
    } else if token == OsStr::new("version") {
        commands::self_version::run(ctx, cli::self_version::SelfVersionArgs)
    } else if token == OsStr::new("config-status") {
        commands::self_config_status::run(ctx, cli::self_config_status::SelfConfigStatusArgs)
    } else if token == OsStr::new("config-merge") {
        commands::self_config_merge::run(ctx, cli::self_config_merge::SelfConfigMergeArgs)
    } else if token == OsStr::new("show-local") {
        commands::self_show_local::run(ctx, cli::self_show_local::SelfShowLocalArgs)
    } else {
        Err(error::AppError::UnknownSelfVerb(
            token.to_string_lossy().into_owned(),
        ))
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
