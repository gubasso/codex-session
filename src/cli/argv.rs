//! Pre-parse argv inspection.
//!
//! What this is: argv-only helpers that run *before* `Cli::try_parse_from`.
//! They normalize a legacy quirk and let `main` reject `self <verb>` with
//! `EX_USAGE` (64) instead of letting `external_subcommand` swallow it.
//!
//! What this is not: clap parsing — that stays in `cli/mod.rs`.

use std::ffi::OsString;

use camino::Utf8PathBuf;

use crate::{cli::OutputFormat, config::LogFormat};

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
            "--verbose" | "--log-stderr" | "--quiet" | "-q" | "--silent" | "--version" | "-V"
            | "--dry-run" => {}
            "--config" | "--log-format" | "--format" => {
                let _ = iter.next();
            }
            _ if s.starts_with("--config=") => {}
            _ if s.starts_with("--log-format=") => {}
            _ if s.starts_with("--format=") => {}
            "self" => return true,
            _ if s.strip_prefix('-').is_some_and(|rest| {
                !rest.is_empty() && rest.chars().all(|ch| ch == 'v' || ch == 'q')
            }) => {}
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
        if s == "--quiet" || s == "-q" {
            global.quiet = true;
            continue;
        }
        if s == "--silent" {
            global.silent = true;
            continue;
        }
        if s == "--log-format" {
            global.log_format = iter.next().and_then(parse_log_format_arg);
            continue;
        }
        if s == "--version" || s == "-V" {
            global.version = true;
            continue;
        }
        if s == "--format" {
            global.format = iter.next().and_then(parse_output_format_arg);
            continue;
        }
        if s == "--dry-run" {
            global.dry_run = true;
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
        if let Some(value) = s.strip_prefix("--log-format=") {
            global.log_format = parse_log_format_str(value);
            continue;
        }
        if let Some(value) = s.strip_prefix("--format=") {
            global.format = parse_output_format_str(value);
            continue;
        }
        if let Some(rest) = s.strip_prefix('-') {
            if !rest.is_empty() && rest.chars().all(|ch| ch == 'v' || ch == 'q') {
                for ch in rest.chars() {
                    match ch {
                        'v' => global.verbose = global.verbose.saturating_add(1),
                        'q' => global.quiet = true,
                        _ => {}
                    }
                }
            }
        }
    }
    global
}

fn parse_log_format_arg(value: &OsString) -> Option<LogFormat> {
    value.to_str().and_then(parse_log_format_str)
}

fn parse_log_format_str(value: &str) -> Option<LogFormat> {
    match value {
        "json" => Some(LogFormat::Json),
        "pretty" => Some(LogFormat::Pretty),
        _ => None,
    }
}

fn parse_output_format_arg(value: &OsString) -> Option<OutputFormat> {
    value.to_str().and_then(parse_output_format_str)
}

fn parse_output_format_str(value: &str) -> Option<OutputFormat> {
    match value {
        "text" => Some(OutputFormat::Text),
        "json" => Some(OutputFormat::Json),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{legacy_self_invocation, scan_global_args};
    use crate::config::LogFormat;
    use std::ffi::OsString;

    #[test]
    fn scan_global_args_reads_new_logging_flags() {
        let argv = vec![
            OsString::from("--silent"),
            OsString::from("-q"),
            OsString::from("--log-format=json"),
            OsString::from("--format=text"),
        ];
        let global = scan_global_args(&argv);
        assert!(global.silent);
        assert!(global.quiet);
        assert_eq!(global.log_format, Some(LogFormat::Json));
        assert_eq!(global.format, Some(crate::cli::OutputFormat::Text));
    }

    #[test]
    fn scan_global_args_reads_space_separated_log_format() {
        let argv = vec![
            OsString::from("--log-format"),
            OsString::from("pretty"),
            OsString::from("-vv"),
        ];
        let global = scan_global_args(&argv);
        assert_eq!(global.log_format, Some(LogFormat::Pretty));
        assert_eq!(global.verbose, 2);
    }

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
}
