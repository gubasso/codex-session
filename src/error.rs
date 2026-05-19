//! Crate-level error type and exit-code mapping.
#![allow(clippy::must_use_candidate)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Application-wide error type.
#[derive(Debug, thiserror::Error)]
pub(crate) enum AppError {
    /// Usage or clap parsing failure.
    #[error("{0}")]
    Usage(String),

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
    BaseMissing(std::path::PathBuf),

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

impl AppError {
    /// Stable machine-readable error kind.
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::Usage(_) => "usage",
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
            Self::Other(err) if is_config_marker(err) => "config",
            Self::Other(_) => "internal",
        }
    }

    /// Tag a `ConfigError` so `exit_code` returns 78 without a dedicated variant yet.
    pub(crate) fn from_config_error(err: crate::config::ConfigError) -> Self {
        Self::Other(anyhow::Error::new(err).context("config-error"))
    }

    pub(crate) fn from_spawner_error(err: crate::adapters::spawner::SpawnerError) -> Self {
        use crate::adapters::spawner::SpawnerError;
        match err {
            SpawnerError::NotFound {
                tried,
                path_searched,
            } => Self::ChildNotFound {
                tried: tried.into_std_path_buf(),
                path_searched,
            },
            SpawnerError::NotExecutable { path } => Self::ChildNotExecutable {
                path: path.into_std_path_buf(),
            },
            SpawnerError::Recursion { path } => Self::ChildRecursion { path },
            SpawnerError::Exec(io) => Self::ChildExec(io),
            SpawnerError::NonUtf8Path(err) => Self::Other(anyhow::Error::new(err)),
        }
    }

    pub(crate) fn from_spawner_error_ref(err: &crate::adapters::spawner::SpawnerError) -> Self {
        use crate::adapters::spawner::SpawnerError;
        match err {
            SpawnerError::NotFound {
                tried,
                path_searched,
            } => Self::ChildNotFound {
                tried: tried.clone().into_std_path_buf(),
                path_searched: path_searched.clone(),
            },
            SpawnerError::NotExecutable { path } => Self::ChildNotExecutable {
                path: path.clone().into_std_path_buf(),
            },
            SpawnerError::Recursion { path } => Self::ChildRecursion { path: path.clone() },
            SpawnerError::Exec(io) => {
                Self::ChildExec(std::io::Error::new(io.kind(), io.to_string()))
            }
            SpawnerError::NonUtf8Path(_) => Self::Other(anyhow::anyhow!("non-utf8 child path")),
        }
    }

    /// Convert to the process exit code.
    pub(crate) fn exit_code(&self) -> u8 {
        match self {
            Self::Usage(_) => 64,
            Self::ChildNotFound { .. } => 127,
            Self::ChildNotExecutable { .. } => 126,
            Self::BaseMissing(_) => 66,
            Self::Fs(err) | Self::Merge(crate::services::merge::MergeError::Fs(err)) => {
                fs_error_exit_code(err)
            }
            Self::Io(err) if err.kind() == std::io::ErrorKind::NotFound => 66,
            Self::Io(err) if err.kind() == std::io::ErrorKind::PermissionDenied => 77,
            Self::ChildExec(_) | Self::Io(_) => 74,
            Self::Other(err) if is_config_marker(err) => 78,
            Self::ChildRecursion { .. } | Self::Other(_) => 70,
        }
    }
}

/// Render an application error to a user-facing writer.
pub(crate) fn render(mut out: impl std::io::Write, err: &AppError) -> std::io::Result<()> {
    if let AppError::Usage(message) = err {
        return out.write_all(message.as_bytes());
    }

    let use_color = crate::ui::color::stderr_color();
    let detail = detail(err);
    writeln!(
        out,
        "{} {}",
        style_label("codex-session:", use_color),
        detail.what
    )?;
    if let Some(where_line) = detail.where_line {
        writeln!(out, "  {} {where_line}", style_label("where:", use_color))?;
    }
    writeln!(
        out,
        "  {} {}",
        style_label("why:  ", use_color),
        detail.why_line
    )?;
    if let Some(hint) = detail.hint_line {
        writeln!(out, "  {} {hint}", style_label("hint: ", use_color))?;
    }

    // Walk the `source()` chain. Per `cli-design/02-error-messages.md`:
    // "Dedupe — if a wrapper's message is `caused by: <inner.message>`,
    // don't print the inner twice." Several `AppError` variants set
    // `why_line = source.to_string()` (`Process`, `Merge`, `Io`, `Other`),
    // which would make the very first `caused by:` line a verbatim repeat
    // of `why:`. Suppress those exact duplicates.
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

pub(crate) fn render_error(err: &AppError) -> std::io::Result<()> {
    let mut stderr = std::io::stderr().lock();
    render(&mut stderr, err)
}

/// Emit the structured log record for an application error.
pub(crate) fn log_error(err: &AppError) {
    let detail = detail(err);
    tracing::error!(
        op = "command.error",
        status = "error",
        err.kind = err.kind(),
        err.msg = detail.why_line,
        error.what = detail.what,
        error.where = detail.where_line.unwrap_or_default(),
        error.hint = detail.hint_line.unwrap_or_default(),
    );
}

struct ErrorDetail {
    what: &'static str,
    where_line: Option<String>,
    why_line: String,
    hint_line: Option<&'static str>,
}

fn detail(err: &AppError) -> ErrorDetail {
    match err {
        AppError::Usage(message) => ErrorDetail {
            what: "invalid command usage",
            where_line: None,
            why_line: message.trim().to_owned(),
            hint_line: None,
        },
        AppError::ChildNotFound {
            tried,
            path_searched,
        } => ErrorDetail {
            what: "failed to resolve wrapped codex binary",
            where_line: Some(path_searched.as_ref().map_or_else(
                || tried.display().to_string(),
                |path| format!("{} (PATH={})", tried.display(), Path::new(path).display()),
            )),
            why_line: "the configured child binary could not be found".to_owned(),
            hint_line: Some("set CODEX_SESSION_CHILD_BIN or add codex to PATH and retry"),
        },
        AppError::ChildNotExecutable { path } => ErrorDetail {
            what: "failed to execute wrapped codex binary",
            where_line: Some(path.display().to_string()),
            why_line: "the configured child binary exists but is not executable".to_owned(),
            hint_line: Some(
                "chmod +x the child binary or point CODEX_SESSION_CHILD_BIN at an executable file",
            ),
        },
        AppError::BaseMissing(path) => ErrorDetail {
            what: "failed to load base config",
            where_line: Some(path.display().to_string()),
            why_line: "the base config file does not exist".to_owned(),
            hint_line: Some("create ~/.codex/config.base.toml or skip the forced merge command"),
        },
        AppError::ChildExec(source) => ErrorDetail {
            what: "failed to hand control to the wrapped codex process",
            where_line: None,
            why_line: source.to_string(),
            hint_line: None,
        },
        AppError::ChildRecursion { path } => ErrorDetail {
            what: "child binary resolves to the wrapper itself",
            where_line: Some(path.to_string()),
            why_line: "the resolved child path is the wrapper binary; this would loop forever"
                .to_owned(),
            hint_line: Some("unset CODEX_SESSION_CHILD_BIN or point it at the real `codex`"),
        },
        AppError::Fs(source) => fs_error_detail(source),
        AppError::Merge(source) => ErrorDetail {
            what: "failed to merge codex config files",
            where_line: None,
            why_line: source.to_string(),
            hint_line: None,
        },
        AppError::Io(source) => ErrorDetail {
            what: "unexpected I/O failure",
            where_line: None,
            why_line: source.to_string(),
            hint_line: None,
        },
        AppError::Other(source) if is_config_marker(source) => config_error_detail(source),
        AppError::Other(source) => ErrorDetail {
            what: "internal wrapper failure",
            where_line: None,
            why_line: source.to_string(),
            hint_line: None,
        },
    }
}

fn style_label(label: &str, use_color: bool) -> String {
    if use_color {
        format!("\u{1b}[1m{label}\u{1b}[0m")
    } else {
        label.to_owned()
    }
}

fn config_error_detail(err: &anyhow::Error) -> ErrorDetail {
    // Walk the chain past the `config-error` marker context and emit the
    // root cause as `why`. Without this branch, `why` reads `config-error`
    // and the user sees "internal wrapper failure" for what is in fact a
    // config problem.
    let why_line = err
        .chain()
        .find(|cause| cause.to_string() != "config-error")
        .map_or_else(|| err.to_string(), ToString::to_string);
    ErrorDetail {
        what: "configuration error",
        where_line: None,
        why_line,
        hint_line: Some(
            "check your config files under $XDG_CONFIG_HOME/codex-session and ./.codex-session",
        ),
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

fn fs_error_detail(err: &crate::adapters::fs::FsError) -> ErrorDetail {
    use crate::adapters::fs::FsError;
    match err {
        FsError::Read { path, source } => ErrorDetail {
            what: "failed to read a file",
            where_line: Some(path.display().to_string()),
            why_line: source.to_string(),
            hint_line: None,
        },
        FsError::Write { path, source } => ErrorDetail {
            what: "failed to write a file",
            where_line: Some(path.display().to_string()),
            why_line: source.to_string(),
            hint_line: None,
        },
        FsError::Mkdir { path, source } => ErrorDetail {
            what: "failed to create a directory",
            where_line: Some(path.display().to_string()),
            why_line: source.to_string(),
            hint_line: None,
        },
        FsError::Stat { path, source } => ErrorDetail {
            what: "failed to inspect a filesystem path",
            where_line: Some(path.display().to_string()),
            why_line: source.to_string(),
            hint_line: None,
        },
        FsError::Touch { path, source } => ErrorDetail {
            what: "failed to update the merge stamp",
            where_line: Some(path.display().to_string()),
            why_line: source.to_string(),
            hint_line: None,
        },
    }
}

fn is_config_marker(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| cause.to_string() == "config-error")
}

#[cfg(test)]
mod tests {
    use super::AppError;

    #[test]
    fn usage_is_sixty_four() {
        assert_eq!(AppError::Usage(String::from("bad")).exit_code(), 64);
    }

    #[test]
    fn child_not_found_is_one_twenty_seven() {
        assert_eq!(
            AppError::ChildNotFound {
                tried: std::path::PathBuf::from("codex"),
                path_searched: Some(std::ffi::OsString::from("/usr/bin:/bin")),
            }
            .exit_code(),
            127
        );
    }

    #[test]
    fn child_not_executable_is_one_twenty_six() {
        assert_eq!(
            AppError::ChildNotExecutable {
                path: std::path::PathBuf::from("/tmp/codex"),
            }
            .exit_code(),
            126
        );
    }

    #[test]
    fn base_missing_is_sixty_six() {
        assert_eq!(
            AppError::BaseMissing(std::path::PathBuf::from("/tmp/base")).exit_code(),
            66
        );
    }

    #[test]
    fn child_exec_is_seventy_four() {
        assert_eq!(
            AppError::ChildExec(std::io::Error::from(std::io::ErrorKind::Other)).exit_code(),
            74
        );
    }

    #[test]
    fn child_recursion_is_seventy() {
        assert_eq!(
            AppError::ChildRecursion {
                path: camino::Utf8PathBuf::from("/usr/local/bin/codex-session"),
            }
            .exit_code(),
            70
        );
    }

    #[test]
    fn io_not_found_is_sixty_six() {
        assert_eq!(
            AppError::Io(std::io::Error::from(std::io::ErrorKind::NotFound)).exit_code(),
            66
        );
    }

    #[test]
    fn io_permission_denied_is_seventy_seven() {
        assert_eq!(
            AppError::Io(std::io::Error::from(std::io::ErrorKind::PermissionDenied)).exit_code(),
            77
        );
    }

    #[test]
    fn io_other_is_seventy_four() {
        assert_eq!(
            AppError::Io(std::io::Error::from(std::io::ErrorKind::Other)).exit_code(),
            74
        );
    }

    #[test]
    fn fs_read_not_found_is_sixty_six() {
        assert_eq!(
            AppError::Fs(crate::adapters::fs::FsError::Read {
                path: std::path::PathBuf::from("/tmp/missing"),
                source: std::io::Error::from(std::io::ErrorKind::NotFound),
            })
            .exit_code(),
            66
        );
    }

    #[test]
    fn fs_read_permission_denied_is_seventy_seven() {
        assert_eq!(
            AppError::Fs(crate::adapters::fs::FsError::Read {
                path: std::path::PathBuf::from("/tmp/secret"),
                source: std::io::Error::from(std::io::ErrorKind::PermissionDenied),
            })
            .exit_code(),
            77
        );
    }

    #[test]
    fn fs_write_is_seventy_four() {
        assert_eq!(
            AppError::Fs(crate::adapters::fs::FsError::Write {
                path: std::path::PathBuf::from("/tmp/out"),
                source: std::io::Error::from(std::io::ErrorKind::Other),
            })
            .exit_code(),
            74
        );
    }

    #[test]
    fn merge_fs_write_other_is_seventy_four() {
        assert_eq!(
            AppError::Merge(crate::services::merge::MergeError::Fs(
                crate::adapters::fs::FsError::Write {
                    path: std::path::PathBuf::from("/tmp/out"),
                    source: std::io::Error::from(std::io::ErrorKind::Other),
                },
            ))
            .exit_code(),
            74
        );
    }

    #[test]
    fn merge_fs_read_not_found_is_sixty_six() {
        assert_eq!(
            AppError::Merge(crate::services::merge::MergeError::Fs(
                crate::adapters::fs::FsError::Read {
                    path: std::path::PathBuf::from("/tmp/missing"),
                    source: std::io::Error::from(std::io::ErrorKind::NotFound),
                },
            ))
            .exit_code(),
            66
        );
    }

    #[test]
    fn merge_fs_write_permission_denied_is_seventy_seven() {
        assert_eq!(
            AppError::Merge(crate::services::merge::MergeError::Fs(
                crate::adapters::fs::FsError::Write {
                    path: std::path::PathBuf::from("/tmp/out"),
                    source: std::io::Error::from(std::io::ErrorKind::PermissionDenied),
                },
            ))
            .exit_code(),
            77
        );
    }

    #[test]
    fn other_is_seventy() {
        assert_eq!(AppError::Other(anyhow::anyhow!("boom")).exit_code(), 70);
    }

    #[test]
    fn config_marker_is_seventy_eight() {
        let err = AppError::Other(anyhow::anyhow!("boom").context("config-error"));
        assert_eq!(err.exit_code(), 78);
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    fn render_dedupes_caused_by_when_why_matches_first_source() {
        // Wrapped variants like `ChildExec` set `why_line = source.to_string()`.
        // The first `caused by:` line would repeat the same message; the
        // renderer must suppress that duplicate.
        let err = AppError::ChildExec(std::io::Error::other("exec boom"));
        let mut buf = Vec::new();
        super::render(&mut buf, &err).unwrap();
        let rendered = String::from_utf8(buf).unwrap();
        assert!(
            rendered.contains("why:"),
            "render must include a why line: {rendered}"
        );
        let caused_by_count = rendered.matches("caused by:").count();
        // `ChildExec` stores the inner `io::Error` directly, so `why:` and the
        // first source message are identical. The renderer should suppress the
        // duplicate entirely.
        assert_eq!(
            caused_by_count, 0,
            "expected zero caused-by lines after dedupe, got {caused_by_count} in:\n{rendered}"
        );
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    fn render_dedupes_when_other_message_equals_source_message() {
        // `AppError::Other` is an `anyhow::Error`; its source chain echoes
        // the same string used in `why_line`. Verify no duplicate caused-by.
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
