//! Pre-parse argv inspection.
//!
//! What this is: argv-only helpers that run *before* `Cli::try_parse_from`.
//! They normalize a legacy quirk and let `main` reject `self <verb>` with
//! `EX_USAGE` (64) instead of letting `external_subcommand` swallow it.
//!
//! What this is not: clap parsing — that stays in `cli/mod.rs`.

use std::ffi::OsString;

use camino::Utf8PathBuf;

use super::GlobalArgs;

/// Strip a stray empty argument that follows the legacy `self` token.
pub(crate) fn normalize_argv(mut argv: Vec<OsString>) -> Vec<OsString> {
    if argv.first().and_then(|token| token.to_str()) == Some("self")
        && argv.get(1).is_some_and(|token| token.is_empty())
    {
        argv.remove(1);
    }
    argv
}

/// Detect a legacy `codex-session ... self <verb>` invocation.
///
/// `clap`'s `external_subcommand` would otherwise treat `self` as a
/// passthrough verb; this helper lets `main` short-circuit and emit the
/// canonical "unrecognized subcommand" usage error with exit code 64.
pub(crate) fn legacy_self_invocation(argv: &[OsString]) -> bool {
    let mut iter = argv.iter();
    while let Some(token) = iter.next() {
        let Some(s) = token.to_str() else {
            return false;
        };
        match s {
            "--" => return false,
            "--verbose" | "--log-stderr" | "--version" => {}
            "--config" => {
                let _ = iter.next();
            }
            _ if s.starts_with("--config=") => {}
            "self" => return true,
            _ if s
                .strip_prefix('-')
                .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|ch| ch == 'v')) => {}
            _ => return false,
        }
    }
    false
}

/// Recover a best-effort `GlobalArgs` view from raw argv.
///
/// Used by `main` only when `Cli::try_parse_from` already failed and we
/// still need to honor `--verbose`, `--log-stderr`, `--config`, and
/// `--version` for diagnostics/logging on the error path.
pub(crate) fn scan_global_args(argv: &[OsString]) -> GlobalArgs {
    let mut global = GlobalArgs::default();
    let mut iter = argv.iter();
    while let Some(token) = iter.next() {
        let Some(s) = token.to_str() else { continue };
        if s == "--verbose" {
            global.verbose = global.verbose.saturating_add(1);
            continue;
        }
        if s == "--log-stderr" {
            global.log_stderr = true;
            continue;
        }
        if s == "--version" {
            global.version = true;
            continue;
        }
        if s == "--config" {
            global.config = iter
                .next()
                .and_then(|value| value.to_str().map(Utf8PathBuf::from));
            continue;
        }
        if let Some(path) = s.strip_prefix("--config=") {
            global.config = Some(Utf8PathBuf::from(path));
            continue;
        }
        if let Some(rest) = s.strip_prefix('-') {
            if !rest.is_empty() && rest.chars().all(|ch| ch == 'v') {
                global.verbose = global
                    .verbose
                    .saturating_add(u8::try_from(rest.len()).unwrap_or(u8::MAX));
            }
        }
    }
    global
}
