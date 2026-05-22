//! Pre-parse argv inspection.
//!
//! What this is: argv-only helpers that run *before* `Cli::try_parse_from`.
//! They normalize a legacy quirk and let `main` reject `self <verb>` with
//! `EX_USAGE` (64) instead of letting `external_subcommand` swallow it.
//!
//! What this is not: clap parsing — that stays in `cli/mod.rs`.

use std::ffi::OsString;

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
            "--verbose" | "--log-stderr" | "--quiet" | "-q" | "--silent" | "--version" | "-V"
            | "--dry-run" => {}
            "--config" | "--log-format" | "--format" | "--profile" | "--group" | "--account" => {
                let _ = iter.next();
            }
            _ if s.starts_with("--config=") => {}
            _ if s.starts_with("--log-format=") => {}
            _ if s.starts_with("--format=") => {}
            _ if s.starts_with("--profile=") => {}
            _ if s.starts_with("--group=") => {}
            _ if s.starts_with("--account=") => {}
            "self" => return true,
            _ if s.strip_prefix('-').is_some_and(|rest| {
                !rest.is_empty() && rest.chars().all(|ch| ch == 'v' || ch == 'q')
            }) => {}
            _ => return false,
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{legacy_self_invocation, normalize_argv};
    use std::ffi::OsString;

    #[test]
    fn legacy_self_invocation_survives_new_flags() {
        let argv = vec![
            OsString::from("--silent"),
            OsString::from("--log-format"),
            OsString::from("json"),
            OsString::from("self"),
            OsString::from("version"),
        ];
        assert!(legacy_self_invocation(&argv));
    }

    #[test]
    fn legacy_self_invocation_survives_account_flag() {
        let argv = vec![
            OsString::from("--account"),
            OsString::from("work"),
            OsString::from("--profile=fast"),
            OsString::from("self"),
            OsString::from("version"),
        ];
        assert!(legacy_self_invocation(&argv));
    }

    #[test]
    fn help_token_is_preserved_for_clap() {
        // Bare `help` must reach clap unchanged so clap's auto-generated
        // `help` subcommand emits a `DisplayHelp` early-exit (matching
        // `--help`). The wrapper must not strip it pre-parse — doing so
        // would route help rendering through the dispatch path and force
        // config/logging init, breaking the byte-equal contract with
        // `--help` on systems where the log dir is unwritable.
        let argv = vec![OsString::from("-v"), OsString::from("help")];
        assert_eq!(
            normalize_argv(argv),
            vec![OsString::from("-v"), OsString::from("help")]
        );
    }

    #[test]
    fn help_with_trailing_args_is_preserved() {
        let argv = vec![OsString::from("help"), OsString::from("config")];
        assert_eq!(
            normalize_argv(argv),
            vec![OsString::from("help"), OsString::from("config")]
        );
    }
}
