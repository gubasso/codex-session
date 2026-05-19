//! Crate-level error type and exit-code mapping.
//!
//! What this is: the wrapper's top-level typed error enum plus stderr/log
//! rendering and process-exit mapping.
//! What this is not: command dispatch or config loading.

#![allow(clippy::must_use_candidate)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Application-wide error type.
#[derive(Debug, thiserror::Error)]
pub(crate) enum AppError {
    /// Usage or clap parsing failure.
    #[error(transparent)]
    Usage(#[from] clap::Error),

    /// Configuration loading failure.
    #[error(transparent)]
    Config(#[from] crate::config::ConfigError),

    /// Missing `codex` on `PATH` or via override.
    #[error("failed to resolve wrapped codex binary")]
    ChildNotFound {
        /// Attempted path or program name.
        tried: PathBuf,
        /// PATH value consulted for lookup.
        path_searched: Option<OsString>,
    },

    /// The resolved child path is not executable.
    #[error("wrapped codex binary is not executable")]
    ChildNotExecutable {
        /// Non-executable child path.
        path: PathBuf,
    },

    /// Missing base config for forced merge.
    #[error("failed to load base config")]
    BaseMissing(PathBuf),

    /// Exec failure after the child binary was resolved.
    #[error("exec failed: {0}")]
    ChildExec(#[source] std::io::Error),

    /// Resolved child path equals the wrapper binary itself.
    #[error("child binary resolves to the wrapper itself")]
    ChildRecursion {
        /// Offending path that resolved to the wrapper.
        path: camino::Utf8PathBuf,
    },

    /// Filesystem adapter failure.
    #[error(transparent)]
    Fs(#[from] crate::adapters::fs::FsError),

    /// Merge service failure.
    #[error(transparent)]
    Merge(#[from] crate::services::merge::MergeError),

    /// Unexpected I/O failure.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// Opaque application-edge failure.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl From<crate::adapters::spawner::SpawnerError> for AppError {
    fn from(err: crate::adapters::spawner::SpawnerError) -> Self {
        use crate::adapters::spawner::SpawnerError as E;

        match err {
            E::NotFound {
                tried,
                path_searched,
            } => Self::ChildNotFound {
                tried: tried.into_std_path_buf(),
                path_searched,
            },
            E::NotExecutable { path } => Self::ChildNotExecutable {
                path: path.into_std_path_buf(),
            },
            E::Exec(io) => Self::ChildExec(io),
            E::Recursion { path } => Self::ChildRecursion { path },
            E::NonUtf8Path(err) => Self::Other(anyhow::Error::new(err)),
        }
    }
}

impl AppError {
    /// Return the stable machine-readable error kind.
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::Usage(_) => "usage",
            Self::Config(err) => err.kind(),
            Self::ChildNotFound { .. } => "child-not-found",
            Self::ChildNotExecutable { .. } => "child-not-executable",
            Self::BaseMissing(_) => "base-missing",
            Self::ChildExec(_) => "child-exec",
            Self::ChildRecursion { .. } => "child-recursion",
            Self::Fs(err) => fs_error_kind(err),
            Self::Merge(_) => "merge-failed",
            Self::Io(err) if err.kind() == std::io::ErrorKind::NotFound => "io-not-found",
            Self::Io(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
                "io-permission-denied"
            }
            Self::Io(_) => "io-other",
            Self::Other(_) => "internal",
        }
    }

    /// Convert the application error into a process exit code.
    pub(crate) fn exit_code(&self) -> u8 {
        match self {
            Self::Usage(err) => clap_exit_code(err),
            Self::Config(_) => 78,
            Self::ChildNotFound { .. } => 127,
            Self::ChildNotExecutable { .. } => 126,
            Self::BaseMissing(_) => 66,
            Self::ChildRecursion { .. } | Self::Other(_) => 70,
            Self::Fs(err) | Self::Merge(crate::services::merge::MergeError::Fs(err)) => {
                fs_error_exit_code(err)
            }
            Self::Io(err) if err.kind() == std::io::ErrorKind::NotFound => 66,
            Self::Io(err) if err.kind() == std::io::ErrorKind::PermissionDenied => 77,
            Self::ChildExec(_) | Self::Io(_) => 74,
        }
    }
}

/// Render an application error to a user-facing writer.
pub(crate) fn render(mut out: impl std::io::Write, err: &AppError) -> std::io::Result<()> {
    if let AppError::Usage(clap_err) = err {
        let rendered = clap_err.render().ansi().to_string();
        return out.write_all(rendered.as_bytes());
    }

    let use_color = crate::ui::color::stderr_color();
    let detail = detail(err);
    writeln!(
        out,
        "{} {}",
        style_label("codex-session:", use_color),
        detail.what
    )?;
    if let Some(where_line) = format_where_line(err) {
        writeln!(out, "  {} {where_line}", style_label("where:", use_color))?;
    }
    writeln!(
        out,
        "  {} {}",
        style_label("why:  ", use_color),
        detail.why_line
    )?;
    if let Some(hint) = error_hint(err) {
        writeln!(out, "  {} {hint}", style_label("hint: ", use_color))?;
    }

    let mut prev = detail.why_line.clone();
    let mut source = std::error::Error::source(err);
    while let Some(cause) = source {
        let msg = cause.to_string();
        if msg != prev {
            writeln!(out, "  {} {msg}", style_label("caused by:", use_color))?;
            prev = msg;
        }
        source = cause.source();
    }

    Ok(())
}

/// Render an application error to stderr.
pub(crate) fn render_error(err: &AppError) -> std::io::Result<()> {
    let mut stderr = std::io::stderr().lock();
    render(&mut stderr, err)
}

/// Emit the structured log record for an application error.
pub(crate) fn log_error(err: &AppError) {
    let path = error_path(err);
    let line = error_line(err);
    let hint = error_hint(err);
    tracing::error!(
        op = "command.error",
        status = "error",
        err.kind = err.kind(),
        err.msg = %err,
        err.path = path.as_deref(),
        err.line = line,
        err.hint = hint,
    );
}

/// Log, render, and map an application error into an exit code.
pub(crate) fn print_and_exit(err: &AppError, global: &crate::cli::GlobalArgs) -> ExitCode {
    log_error(err);
    if !global.silent {
        if let AppError::Usage(clap_err) = err {
            let _ = clap_err.print();
        } else {
            let _ = render_error(err);
        }
    }

    ExitCode::from(err.exit_code())
}

struct ErrorDetail {
    what: String,
    why_line: String,
}

fn detail(err: &AppError) -> ErrorDetail {
    match err {
        AppError::Usage(clap_err) => ErrorDetail {
            what: "invalid command usage".to_owned(),
            why_line: clap_err.to_string(),
        },
        AppError::Config(config_err) => config_error_detail(config_err),
        AppError::ChildNotFound {
            tried,
            path_searched,
        } => ErrorDetail {
            what: "failed to resolve wrapped codex binary".to_owned(),
            why_line: path_searched.as_ref().map_or_else(
                || format!("could not find `{}`", tried.display()),
                |path| {
                    format!(
                        "could not find `{}` on PATH={}",
                        tried.display(),
                        Path::new(path).display()
                    )
                },
            ),
        },
        AppError::ChildNotExecutable { path } => ErrorDetail {
            what: "failed to execute wrapped codex binary".to_owned(),
            why_line: format!("`{}` exists but is not executable", path.display()),
        },
        AppError::BaseMissing(_) => ErrorDetail {
            what: "failed to load base config".to_owned(),
            why_line: "the base config file does not exist".to_owned(),
        },
        AppError::ChildExec(source) => ErrorDetail {
            what: "failed to hand control to the wrapped codex process".to_owned(),
            why_line: source.to_string(),
        },
        AppError::ChildRecursion { .. } => ErrorDetail {
            what: "child binary resolves to the wrapper itself".to_owned(),
            why_line: "the resolved child path is the wrapper binary; this would loop forever"
                .to_owned(),
        },
        AppError::Fs(source) => ErrorDetail {
            what: fs_error_what(source).to_owned(),
            why_line: fs_error_why(source),
        },
        AppError::Merge(source) => ErrorDetail {
            what: "failed to merge codex config files".to_owned(),
            why_line: source.to_string(),
        },
        AppError::Io(source) => ErrorDetail {
            what: "unexpected I/O failure".to_owned(),
            why_line: source.to_string(),
        },
        AppError::Other(source) => ErrorDetail {
            what: "internal wrapper failure".to_owned(),
            why_line: source.to_string(),
        },
    }
}

fn config_error_detail(err: &crate::config::ConfigError) -> ErrorDetail {
    use crate::config::ConfigError;

    match err {
        ConfigError::NoXdg => ErrorDetail {
            what: "config: missing XDG directories".to_owned(),
            why_line: "could not resolve HOME/XDG base directories".to_owned(),
        },
        ConfigError::CurrentDir(source) => ErrorDetail {
            what: "config: failed to read current working directory".to_owned(),
            why_line: source.to_string(),
        },
        ConfigError::Parse { source, .. } => ErrorDetail {
            what: "config: parse error".to_owned(),
            why_line: source.to_string(),
        },
        ConfigError::UnknownKey { key, .. } => ErrorDetail {
            what: format!("config: unknown key `{key}`"),
            why_line: format!("`{key}` is not a recognized configuration key"),
        },
        ConfigError::ExplicitConfigMissing(_) => ErrorDetail {
            what: "config: explicit config file does not exist".to_owned(),
            why_line: "the file referenced by --config or CODEX_SESSION_CONFIG was not found"
                .to_owned(),
        },
        ConfigError::NonUtf8Path(source) => ErrorDetail {
            what: "config: non-utf8 path".to_owned(),
            why_line: source.to_string(),
        },
        ConfigError::Io(source) => ErrorDetail {
            what: "config: io error".to_owned(),
            why_line: source.to_string(),
        },
        ConfigError::EnvParse {
            key,
            value,
            expected,
        } => ErrorDetail {
            what: format!("config: invalid environment override {key}={value}"),
            why_line: format!("expected {expected}"),
        },
    }
}

fn format_where_line(err: &AppError) -> Option<String> {
    let path = error_path(err)?;
    match error_line(err) {
        Some(line) => Some(format!("{path} (line {line})")),
        None => Some(path),
    }
}

fn error_path(err: &AppError) -> Option<String> {
    use crate::adapters::fs::FsError;
    use crate::config::ConfigError;

    match err {
        AppError::Config(
            ConfigError::Parse { path, .. }
            | ConfigError::UnknownKey { path, .. }
            | ConfigError::ExplicitConfigMissing(path),
        )
        | AppError::ChildRecursion { path } => Some(path.to_string()),
        AppError::ChildNotFound { tried, .. } => Some(tried.display().to_string()),
        AppError::ChildNotExecutable { path }
        | AppError::BaseMissing(path)
        | AppError::Fs(
            FsError::Read { path, .. }
            | FsError::Write { path, .. }
            | FsError::Mkdir { path, .. }
            | FsError::Stat { path, .. }
            | FsError::Touch { path, .. },
        )
        | AppError::Merge(crate::services::merge::MergeError::Fs(
            FsError::Read { path, .. }
            | FsError::Write { path, .. }
            | FsError::Mkdir { path, .. }
            | FsError::Stat { path, .. }
            | FsError::Touch { path, .. },
        )) => Some(path.display().to_string()),
        AppError::Usage(_)
        | AppError::ChildExec(_)
        | AppError::Io(_)
        | AppError::Other(_)
        | AppError::Config(
            ConfigError::NoXdg
            | ConfigError::CurrentDir(_)
            | ConfigError::NonUtf8Path(_)
            | ConfigError::Io(_)
            | ConfigError::EnvParse { .. },
        ) => None,
    }
}

fn error_line(err: &AppError) -> Option<u32> {
    let crate::config::ConfigError::Parse { source, .. } = (match err {
        AppError::Config(config_err) => config_err,
        _ => return None,
    }) else {
        return None;
    };

    let rendered = source.to_string();
    let marker = "line ";
    let start = rendered.find(marker)? + marker.len();
    let digits: String = rendered[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    (!digits.is_empty()).then(|| digits.parse().ok()).flatten()
}

const fn error_hint(err: &AppError) -> Option<&'static str> {
    use crate::config::ConfigError;

    match err {
        AppError::ChildNotFound { .. } => {
            Some("set CODEX_SESSION_CHILD_BIN or add codex to PATH and retry")
        }
        AppError::ChildNotExecutable { .. } => {
            Some("chmod +x the child binary or point CODEX_SESSION_CHILD_BIN at an executable file")
        }
        AppError::BaseMissing(_) => {
            Some("create ~/.codex/config.base.toml or skip the forced merge command")
        }
        AppError::ChildRecursion { .. } => {
            Some("unset CODEX_SESSION_CHILD_BIN or point it at the real `codex`")
        }
        AppError::Config(ConfigError::UnknownKey { .. }) => {
            Some("run `codex-session config status` to see the schema")
        }
        AppError::Config(ConfigError::ExplicitConfigMissing(_)) => {
            Some("pass --config PATH to an existing file or remove the override")
        }
        AppError::Config(
            ConfigError::NoXdg
            | ConfigError::CurrentDir(_)
            | ConfigError::Parse { .. }
            | ConfigError::NonUtf8Path(_)
            | ConfigError::Io(_)
            | ConfigError::EnvParse { .. },
        )
        | AppError::Usage(_)
        | AppError::ChildExec(_)
        | AppError::Fs(_)
        | AppError::Merge(_)
        | AppError::Io(_)
        | AppError::Other(_) => None,
    }
}

fn style_label(label: &str, use_color: bool) -> String {
    if use_color {
        format!("\u{1b}[1m{label}\u{1b}[0m")
    } else {
        label.to_owned()
    }
}

fn clap_exit_code(err: &clap::Error) -> u8 {
    use clap::error::ErrorKind;

    match err.kind() {
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => 0,
        _ => 64,
    }
}

fn fs_error_exit_code(err: &crate::adapters::fs::FsError) -> u8 {
    use crate::adapters::fs::FsError;

    match err {
        FsError::Read { source, .. } if source.kind() == std::io::ErrorKind::NotFound => 66,
        FsError::Read { source, .. } if source.kind() == std::io::ErrorKind::PermissionDenied => 77,
        FsError::Stat { source, .. } if source.kind() == std::io::ErrorKind::NotFound => 66,
        FsError::Stat { source, .. } if source.kind() == std::io::ErrorKind::PermissionDenied => 77,
        FsError::Write { source, .. }
        | FsError::Mkdir { source, .. }
        | FsError::Touch { source, .. }
            if source.kind() == std::io::ErrorKind::PermissionDenied =>
        {
            77
        }
        FsError::Read { .. }
        | FsError::Write { .. }
        | FsError::Mkdir { .. }
        | FsError::Stat { .. }
        | FsError::Touch { .. } => 74,
    }
}

const fn fs_error_kind(err: &crate::adapters::fs::FsError) -> &'static str {
    use crate::adapters::fs::FsError;

    match err {
        FsError::Read { .. } => "fs-read",
        FsError::Write { .. } => "fs-write",
        FsError::Mkdir { .. } => "fs-mkdir",
        FsError::Stat { .. } => "fs-stat",
        FsError::Touch { .. } => "fs-touch",
    }
}

const fn fs_error_what(err: &crate::adapters::fs::FsError) -> &'static str {
    use crate::adapters::fs::FsError;

    match err {
        FsError::Read { .. } => "failed to read a file",
        FsError::Write { .. } => "failed to write a file",
        FsError::Mkdir { .. } => "failed to create a directory",
        FsError::Stat { .. } => "failed to inspect a filesystem path",
        FsError::Touch { .. } => "failed to update the merge stamp",
    }
}

fn fs_error_why(err: &crate::adapters::fs::FsError) -> String {
    use crate::adapters::fs::FsError;

    match err {
        FsError::Read { source, .. }
        | FsError::Write { source, .. }
        | FsError::Mkdir { source, .. }
        | FsError::Stat { source, .. }
        | FsError::Touch { source, .. } => source.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::AppError;
    use crate::config::ConfigError;

    fn usage_error() -> clap::Error {
        clap::Error::raw(clap::error::ErrorKind::InvalidSubcommand, "bad")
    }

    #[test]
    fn usage_is_sixty_four() {
        assert_eq!(AppError::Usage(usage_error()).exit_code(), 64);
        assert_eq!(AppError::Usage(usage_error()).kind(), "usage");
    }

    #[test]
    fn config_no_xdg_is_seventy_eight() {
        let err = AppError::Config(ConfigError::NoXdg);
        assert_eq!(err.exit_code(), 78);
        assert_eq!(err.kind(), "config-no-xdg");
    }

    #[test]
    fn config_parse_is_seventy_eight() {
        let err = AppError::Config(ConfigError::Parse {
            path: camino::Utf8PathBuf::from("/tmp/config.toml"),
            source: figment::Error::from("bad config"),
        });
        assert_eq!(err.exit_code(), 78);
        assert_eq!(err.kind(), "config-parse");
    }

    #[test]
    fn config_unknown_key_is_seventy_eight() {
        let err = AppError::Config(ConfigError::UnknownKey {
            key: "bogus".to_owned(),
            path: camino::Utf8PathBuf::from("/tmp/config.toml"),
        });
        assert_eq!(err.exit_code(), 78);
        assert_eq!(err.kind(), "config-unknown-key");
    }

    #[test]
    fn config_explicit_missing_is_seventy_eight() {
        let err = AppError::Config(ConfigError::ExplicitConfigMissing(
            camino::Utf8PathBuf::from("/tmp/missing.toml"),
        ));
        assert_eq!(err.exit_code(), 78);
        assert_eq!(err.kind(), "config-explicit-missing");
    }

    #[test]
    fn config_env_parse_is_seventy_eight() {
        let err = AppError::Config(ConfigError::EnvParse {
            key: "CODEX_SESSION_LOG_VERBOSE".to_owned(),
            value: "abc".to_owned(),
            expected: "u8",
        });
        assert_eq!(err.exit_code(), 78);
        assert_eq!(err.kind(), "config-env-parse");
    }

    #[test]
    fn child_not_found_is_one_twenty_seven() {
        let err = AppError::ChildNotFound {
            tried: std::path::PathBuf::from("codex"),
            path_searched: Some(std::ffi::OsString::from("/usr/bin:/bin")),
        };
        assert_eq!(err.exit_code(), 127);
        assert_eq!(err.kind(), "child-not-found");
    }

    #[test]
    fn child_not_executable_is_one_twenty_six() {
        let err = AppError::ChildNotExecutable {
            path: std::path::PathBuf::from("/tmp/codex"),
        };
        assert_eq!(err.exit_code(), 126);
        assert_eq!(err.kind(), "child-not-executable");
    }

    #[test]
    fn base_missing_is_sixty_six() {
        let err = AppError::BaseMissing(std::path::PathBuf::from("/tmp/base"));
        assert_eq!(err.exit_code(), 66);
        assert_eq!(err.kind(), "base-missing");
    }

    #[test]
    fn child_exec_is_seventy_four() {
        let err = AppError::ChildExec(std::io::Error::from(std::io::ErrorKind::Other));
        assert_eq!(err.exit_code(), 74);
        assert_eq!(err.kind(), "child-exec");
    }

    #[test]
    fn child_recursion_is_seventy() {
        let err = AppError::ChildRecursion {
            path: camino::Utf8PathBuf::from("/usr/local/bin/codex-session"),
        };
        assert_eq!(err.exit_code(), 70);
        assert_eq!(err.kind(), "child-recursion");
    }

    #[test]
    fn io_not_found_is_sixty_six() {
        let err = AppError::Io(std::io::Error::from(std::io::ErrorKind::NotFound));
        assert_eq!(err.exit_code(), 66);
        assert_eq!(err.kind(), "io-not-found");
    }

    #[test]
    fn io_permission_denied_is_seventy_seven() {
        let err = AppError::Io(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        assert_eq!(err.exit_code(), 77);
        assert_eq!(err.kind(), "io-permission-denied");
    }

    #[test]
    fn io_other_is_seventy_four() {
        let err = AppError::Io(std::io::Error::from(std::io::ErrorKind::Other));
        assert_eq!(err.exit_code(), 74);
        assert_eq!(err.kind(), "io-other");
    }

    #[test]
    fn fs_read_not_found_is_sixty_six() {
        let err = AppError::Fs(crate::adapters::fs::FsError::Read {
            path: std::path::PathBuf::from("/tmp/missing"),
            source: std::io::Error::from(std::io::ErrorKind::NotFound),
        });
        assert_eq!(err.exit_code(), 66);
        assert_eq!(err.kind(), "fs-read");
    }

    #[test]
    fn fs_read_permission_denied_is_seventy_seven() {
        let err = AppError::Fs(crate::adapters::fs::FsError::Read {
            path: std::path::PathBuf::from("/tmp/secret"),
            source: std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        });
        assert_eq!(err.exit_code(), 77);
        assert_eq!(err.kind(), "fs-read");
    }

    #[test]
    fn fs_write_is_seventy_four() {
        let err = AppError::Fs(crate::adapters::fs::FsError::Write {
            path: std::path::PathBuf::from("/tmp/out"),
            source: std::io::Error::from(std::io::ErrorKind::Other),
        });
        assert_eq!(err.exit_code(), 74);
        assert_eq!(err.kind(), "fs-write");
    }

    #[test]
    fn merge_fs_write_other_is_seventy_four() {
        let err = AppError::Merge(crate::services::merge::MergeError::Fs(
            crate::adapters::fs::FsError::Write {
                path: std::path::PathBuf::from("/tmp/out"),
                source: std::io::Error::from(std::io::ErrorKind::Other),
            },
        ));
        assert_eq!(err.exit_code(), 74);
        assert_eq!(err.kind(), "merge-failed");
    }

    #[test]
    fn merge_fs_read_not_found_is_sixty_six() {
        let err = AppError::Merge(crate::services::merge::MergeError::Fs(
            crate::adapters::fs::FsError::Read {
                path: std::path::PathBuf::from("/tmp/missing"),
                source: std::io::Error::from(std::io::ErrorKind::NotFound),
            },
        ));
        assert_eq!(err.exit_code(), 66);
        assert_eq!(err.kind(), "merge-failed");
    }

    #[test]
    fn merge_fs_write_permission_denied_is_seventy_seven() {
        let err = AppError::Merge(crate::services::merge::MergeError::Fs(
            crate::adapters::fs::FsError::Write {
                path: std::path::PathBuf::from("/tmp/out"),
                source: std::io::Error::from(std::io::ErrorKind::PermissionDenied),
            },
        ));
        assert_eq!(err.exit_code(), 77);
        assert_eq!(err.kind(), "merge-failed");
    }

    #[test]
    fn other_is_seventy() {
        let err = AppError::Other(anyhow::anyhow!("boom"));
        assert_eq!(err.exit_code(), 70);
        assert_eq!(err.kind(), "internal");
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    fn render_dedupes_caused_by_when_why_matches_first_source() {
        let err = AppError::ChildExec(std::io::Error::other("exec boom"));
        let mut buf = Vec::new();
        super::render(&mut buf, &err).unwrap();
        let rendered = String::from_utf8(buf).unwrap();
        assert!(
            rendered.contains("why:"),
            "render must include a why line: {rendered}"
        );
        let caused_by_count = rendered.matches("caused by:").count();
        assert_eq!(
            caused_by_count, 0,
            "expected zero caused-by lines after dedupe, got {caused_by_count} in:\n{rendered}"
        );
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    fn render_dedupes_when_other_message_equals_source_message() {
        let err = AppError::Other(anyhow::anyhow!("boom"));
        let mut buf = Vec::new();
        super::render(&mut buf, &err).unwrap();
        let rendered = String::from_utf8(buf).unwrap();
        assert!(
            !rendered.contains("caused by: boom\n  caused by: boom"),
            "must not emit consecutive duplicate caused-by lines: {rendered}"
        );
    }
}
