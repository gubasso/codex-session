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

use clap::Parser as _;
use std::ffi::OsString;
use std::io::Write as _;
use std::process::ExitCode;

#[cfg(not(unix))]
compile_error!("codex-session is Unix-only");

fn main() -> ExitCode {
    let argv: Vec<OsString> = std::env::args_os().skip(1).collect();

    // Parse the `self` subtree first so we can honor its verbosity flags from
    // the very first log record, but DO NOT emit any tracing call before the
    // subscriber is installed — parse errors are buffered into a local
    // variable and emitted below, after init, so the error path still
    // produces a structured JSON log record.
    let parse_result = maybe_parse_self_cli(&argv);

    let (self_cli, deferred_early_exit): (Option<cli::SelfCli>, Option<DeferredEarlyExit>) =
        match parse_result {
            Ok(SelfCliParse::NotSelf) => (None, None),
            Ok(SelfCliParse::Parsed(cli)) => (Some(cli), None),
            Ok(SelfCliParse::EarlyExit { message, stdout }) => {
                (None, Some(DeferredEarlyExit::Help { message, stdout }))
            }
            Err(error) => (None, Some(DeferredEarlyExit::Error(error))),
        };
    // When the full `self` parse fails, fall back to a relaxed pre-scan so
    // explicit `-v` / `--log-stderr` flags still control how the error is
    // reported. Only honored under the `self` subtree; top-level pass-through
    // never inspects argv.
    let global = self_cli
        .as_ref()
        .map_or_else(|| scan_self_globals(&argv), |cli| cli.global);
    let paths = domain::paths::CodexPaths::from_env();
    let mirror_stderr = global.log_stderr || global.verbose > 0;
    let _log_init = match logging::init(global.verbose, &paths.log_file, mirror_stderr) {
        Ok(init) => init,
        Err(err) => {
            let app_error =
                error::AppError::Other(anyhow::anyhow!("failed to install tracing: {err}"));
            return print_and_exit(&app_error);
        }
    };
    if paths.log_path_degraded {
        tracing::warn!(
            op = "logging.init",
            status = "degraded",
            log.file = %paths.log_file.display(),
            "HOME and XDG_STATE_HOME were unset; using /tmp log fallback"
        );
    }

    // Handle any deferred parse outcome now that logging is live.
    if let Some(deferred) = deferred_early_exit {
        return match deferred {
            DeferredEarlyExit::Help { message, stdout } => {
                if stdout {
                    let _ = std::io::stdout().lock().write_all(message.as_bytes());
                    ExitCode::SUCCESS
                } else {
                    let app_error = error::AppError::Usage(message);
                    print_and_exit(&app_error)
                }
            }
            DeferredEarlyExit::Error(error) => print_and_exit(&error),
        };
    }

    let ctx = context::AppContext::new(paths);

    let result = self_cli.map_or_else(
        || commands::pass_through::run(&ctx, &argv),
        |self_cli| dispatch_self(&ctx, self_cli),
    );

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => print_and_exit(&e),
    }
}

enum DeferredEarlyExit {
    Help { message: String, stdout: bool },
    Error(error::AppError),
}

enum SelfCliParse {
    NotSelf,
    Parsed(cli::SelfCli),
    EarlyExit { message: String, stdout: bool },
}

fn maybe_parse_self_cli(argv: &[OsString]) -> Result<SelfCliParse, error::AppError> {
    if argv.first().and_then(|s| s.to_str()) != Some("self") {
        return Ok(SelfCliParse::NotSelf);
    }

    let start = if argv
        .get(1)
        .is_some_and(|token| token.as_os_str().is_empty())
    {
        2
    } else {
        1
    };
    let parse_args =
        std::iter::once(OsString::from("codex-session self")).chain(argv[start..].iter().cloned());
    cli::SelfCli::try_parse_from(parse_args).map_or_else(
        |err| match err.kind() {
            clap::error::ErrorKind::DisplayHelp => Ok(SelfCliParse::EarlyExit {
                message: err.render().to_string(),
                stdout: true,
            }),
            _ => Err(error::AppError::Usage(err.render().to_string())),
        },
        |cli| Ok(SelfCliParse::Parsed(cli)),
    )
}

/// Relaxed scan of `self`-subtree global flags for the parse-failure path.
///
/// Mirrors the subset of `cli::SelfGlobalArgs` that we want honored even when
/// the full clap parse fails (so `self -v version junk` and
/// `self version -v junk` both still mirror logs at `info` and write them to
/// stderr, instead of falling back to the default `warn` + file-only).
///
/// `cli::SelfGlobalArgs` is declared `global = true`, so the flags are valid
/// anywhere under the `self` subtree. The scanner therefore inspects every
/// token under `self` rather than stopping at the verb. It will only ever run
/// under the `self` subtree — top-level pass-through never invokes it.
fn scan_self_globals(argv: &[OsString]) -> cli::SelfGlobalArgs {
    let mut g = cli::SelfGlobalArgs::default();
    if argv.first().and_then(|s| s.to_str()) != Some("self") {
        return g;
    }
    for token in argv.iter().skip(1) {
        let Some(s) = token.to_str() else { continue };
        if s == "--verbose" {
            g.verbose = g.verbose.saturating_add(1);
            continue;
        }
        if s == "--log-stderr" {
            g.log_stderr = true;
            continue;
        }
        // Stacked short cluster like `-v`, `-vv`, `-vvvv`. Mirror clap's
        // `ArgAction::Count` semantics: every `v` after the leading `-`
        // increments the counter. Reject any cluster containing a non-`v`
        // character so we never accidentally count flags meant for a wrapped
        // argument (top-level pass-through never invokes this scanner).
        if let Some(rest) = s.strip_prefix('-') {
            if !rest.is_empty() && rest.chars().all(|c| c == 'v') {
                g.verbose = g
                    .verbose
                    .saturating_add(u8::try_from(rest.len()).unwrap_or(u8::MAX));
            }
        }
    }
    g
}

fn dispatch_self(ctx: &context::AppContext, cli: cli::SelfCli) -> Result<(), error::AppError> {
    use cli::SelfCommand;

    match cli
        .command
        .unwrap_or(SelfCommand::Help(crate::cli::self_help::SelfHelpArgs))
    {
        SelfCommand::Help(args) => commands::self_help::run(ctx, args),
        SelfCommand::Version(args) => commands::self_version::run(ctx, args),
        SelfCommand::ConfigStatus(args) => commands::self_config_status::run(ctx, args),
        SelfCommand::ConfigMerge(args) => commands::self_config_merge::run(ctx, args),
        SelfCommand::ShowLocal(args) => commands::self_show_local::run(ctx, args),
    }
}

fn print_and_exit(e: &error::AppError) -> ExitCode {
    error::log_error(e);
    let mut stderr = std::io::stderr().lock();
    let _ = error::render(&mut stderr, e);
    ExitCode::from(e.exit_code())
}

impl error::AppError {
    fn from_process_error(err: crate::adapters::process::ProcessError) -> Self {
        match err {
            crate::adapters::process::ProcessError::NotFound {
                tried,
                path_searched,
            } => Self::ChildNotFound {
                tried,
                path_searched,
            },
            crate::adapters::process::ProcessError::NotExecutable { path } => {
                Self::ChildNotExecutable { path }
            }
            other @ crate::adapters::process::ProcessError::Exec(_) => Self::Process(other),
        }
    }
}
