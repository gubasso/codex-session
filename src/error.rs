//! Crate-level error type and exit-code mapping.
//!
//! What this is: the wrapper's top-level typed error enum plus stderr/log
//! rendering and process-exit mapping.
//! What this is not: command dispatch or config loading.

#![allow(clippy::must_use_candidate)]

use std::ffi::OsString;
use std::os::unix::process::ExitStatusExt as _;
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

    /// Exec failure after the child binary was resolved.
    #[error("exec failed: {0}")]
    ChildExec(#[source] std::io::Error),

    /// Child exited with a non-zero status.
    #[error("child exited with status {0}")]
    ChildExitNonZero(i32),

    /// Child was terminated by a signal.
    #[error("child terminated by signal")]
    ChildSignaled(std::process::ExitStatus),

    /// Resolved child path equals the wrapper binary itself.
    #[error("child binary resolves to the wrapper itself")]
    ChildRecursion {
        /// Offending path that resolved to the wrapper.
        path: camino::Utf8PathBuf,
    },

    /// Auth-bridging validation or lock failure.
    #[error(transparent)]
    Auth(crate::services::auth::AuthError),

    /// Account registry / resolver failure.
    #[error(transparent)]
    Account(crate::services::account::AccountError),

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

impl From<crate::services::auth::AuthError> for AppError {
    fn from(err: crate::services::auth::AuthError) -> Self {
        Self::Auth(err)
    }
}

impl From<crate::services::account::AccountError> for AppError {
    fn from(err: crate::services::account::AccountError) -> Self {
        Self::Account(err)
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
            Self::ChildExec(_) => "child-exec",
            Self::ChildExitNonZero(_) => "child-exit-nonzero",
            Self::ChildSignaled(_) => "child-signaled",
            Self::ChildRecursion { .. } => "child-recursion",
            Self::Auth(err) => err.kind(),
            Self::Account(err) => err.kind(),
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
            Self::ChildExitNonZero(code) => u8::try_from(*code).unwrap_or(u8::MAX),
            Self::ChildSignaled(status) => status
                .signal()
                .and_then(|signal| u8::try_from(128 + signal).ok())
                .unwrap_or(70),
            Self::Account(err) => match err {
                crate::services::account::AccountError::InvalidName { .. } => 64,
                crate::services::account::AccountError::NotFound { .. }
                | crate::services::account::AccountError::AlreadyExists { .. } => 78,
                crate::services::account::AccountError::NoEligible => 75,
                crate::services::account::AccountError::QuotaFetch { .. } => 69,
                crate::services::account::AccountError::QuotaParse { .. } => 65,
                crate::services::account::AccountError::RegistryIo { .. } => 74,
            },
            Self::ChildRecursion { .. } | Self::Other(_) => 70,
            Self::Auth(_) => 75,
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
        AppError::ChildExec(source) => ErrorDetail {
            what: "failed to hand control to the wrapped codex process".to_owned(),
            why_line: source.to_string(),
        },
        AppError::ChildExitNonZero(code) => ErrorDetail {
            what: "wrapped codex exited with a non-zero status".to_owned(),
            why_line: format!("child exited with status {code}"),
        },
        AppError::ChildSignaled(status) => ErrorDetail {
            what: "wrapped codex terminated due to a signal".to_owned(),
            why_line: status.signal().map_or_else(
                || "signal number unavailable".to_owned(),
                |signal| format!("child terminated by signal {signal}"),
            ),
        },
        AppError::ChildRecursion { .. } => ErrorDetail {
            what: "child binary resolves to the wrapper itself".to_owned(),
            why_line: "the resolved child path is the wrapper binary; this would loop forever"
                .to_owned(),
        },
        AppError::Auth(auth_err) => auth_error_detail(auth_err),
        AppError::Account(account_err) => account_error_detail(account_err),
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
        ConfigError::NoHomeDir => ErrorDetail {
            what: "config: home directory could not be resolved".to_owned(),
            why_line: "BaseDirs::new() returned None or the home path was not UTF-8".to_owned(),
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
        ConfigError::ProfileNotFound { name, .. } => ErrorDetail {
            what: format!("config: profile `{name}` not found"),
            why_line: "the requested profile manifest does not exist".to_owned(),
        },
        ConfigError::ManifestParse { .. } => ErrorDetail {
            what: "config: profile manifest parse error".to_owned(),
            why_line: "the YAML manifest could not be parsed".to_owned(),
        },
        ConfigError::ManifestSchema { reason, .. } => ErrorDetail {
            what: "config: invalid profile manifest".to_owned(),
            why_line: reason.clone(),
        },
        ConfigError::LayerNotFound { name, .. } => ErrorDetail {
            what: format!("config: settings layer `{name}` not found"),
            why_line: "the profile references a missing settings layer".to_owned(),
        },
        ConfigError::LayerParse { .. } => ErrorDetail {
            what: "config: settings layer parse error".to_owned(),
            why_line: "the TOML settings layer could not be parsed".to_owned(),
        },
        ConfigError::MergeFailed { reason } => ErrorDetail {
            what: "config: profile composition failed".to_owned(),
            why_line: reason.clone(),
        },
        ConfigError::SessionDirUnresolvable { reason, .. } => ErrorDetail {
            what: "config: secure session directory could not be resolved".to_owned(),
            why_line: reason.clone(),
        },
        ConfigError::EnvKeyInvalid { key, reason } => ErrorDetail {
            what: format!("config: invalid profile env key `{key}`"),
            why_line: reason.clone(),
        },
        ConfigError::AccountConfigParse {
            field,
            value,
            reason,
        } => ErrorDetail {
            what: format!("config: invalid `{field}` value `{value}`"),
            why_line: reason.clone(),
        },
    }
}

fn auth_error_detail(err: &crate::services::auth::AuthError) -> ErrorDetail {
    use crate::services::auth::AuthError;

    match err {
        AuthError::Io { source, .. } => ErrorDetail {
            what: "auth: io error bridging auth.json".to_owned(),
            why_line: source.to_string(),
        },
        AuthError::SymlinkRefused { .. } => ErrorDetail {
            what: "auth: refused symlinked auth.json".to_owned(),
            why_line: "the file is a symbolic link; refusing to follow".to_owned(),
        },
        AuthError::HardlinkRefused { .. } => ErrorDetail {
            what: "auth: refused hardlinked auth.json".to_owned(),
            why_line: "the file has more than one hard link; refusing to use".to_owned(),
        },
        AuthError::BadOwnership { .. } => ErrorDetail {
            what: "auth: bad ownership or permissions on auth.json".to_owned(),
            why_line: "the file must be owned by you and mode 0600".to_owned(),
        },
        AuthError::LockFailed { source, .. } => ErrorDetail {
            what: "auth: failed to acquire auth.json lock".to_owned(),
            why_line: source.to_string(),
        },
        AuthError::JsonParse { source, .. } => ErrorDetail {
            what: "auth: malformed auth.json".to_owned(),
            why_line: source.to_string(),
        },
    }
}

fn account_error_detail(err: &crate::services::account::AccountError) -> ErrorDetail {
    use crate::services::account::AccountError;

    match err {
        AccountError::InvalidName { value, reason } => ErrorDetail {
            what: format!("account: invalid account name `{value}`"),
            why_line: reason.clone(),
        },
        AccountError::NotFound { name, .. } => ErrorDetail {
            what: format!("account: `{name}` not found"),
            why_line: "the named account is not registered".to_owned(),
        },
        AccountError::AlreadyExists { name, .. } => ErrorDetail {
            what: format!("account: `{name}` already exists"),
            why_line: "the named account is already registered".to_owned(),
        },
        AccountError::RegistryIo { source, .. } => ErrorDetail {
            what: "account: registry io error".to_owned(),
            why_line: source.to_string(),
        },
        AccountError::NoEligible => ErrorDetail {
            what: "account: no eligible account".to_owned(),
            why_line: "all accounts were filtered out".to_owned(),
        },
        AccountError::QuotaFetch { detail } => ErrorDetail {
            what: "account: quota fetch failed".to_owned(),
            why_line: detail.clone(),
        },
        AccountError::QuotaParse { detail } => ErrorDetail {
            what: "account: quota parse failed".to_owned(),
            why_line: detail.clone(),
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
    use crate::config::ConfigError;

    match err {
        AppError::Config(
            ConfigError::Parse { path, .. }
            | ConfigError::UnknownKey { path, .. }
            | ConfigError::ExplicitConfigMissing(path)
            | ConfigError::ProfileNotFound { path, .. }
            | ConfigError::ManifestParse { path, .. }
            | ConfigError::ManifestSchema { path, .. }
            | ConfigError::LayerNotFound { path, .. }
            | ConfigError::LayerParse { path, .. },
        )
        | AppError::ChildRecursion { path } => Some(path.to_string()),
        AppError::ChildNotFound { tried, .. } => Some(tried.display().to_string()),
        AppError::ChildNotExecutable { path } => Some(path.display().to_string()),
        AppError::Auth(auth_err) => Some(auth_err.path().to_string()),
        AppError::Account(account_err) => account_err.path().map(ToString::to_string),
        AppError::Usage(_)
        | AppError::ChildExec(_)
        | AppError::ChildExitNonZero(_)
        | AppError::ChildSignaled(_)
        | AppError::Io(_)
        | AppError::Other(_)
        | AppError::Config(
            ConfigError::NoXdg
            | ConfigError::NoHomeDir
            | ConfigError::CurrentDir(_)
            | ConfigError::NonUtf8Path(_)
            | ConfigError::Io(_)
            | ConfigError::EnvParse { .. }
            | ConfigError::MergeFailed { .. }
            | ConfigError::SessionDirUnresolvable { .. }
            | ConfigError::EnvKeyInvalid { .. }
            | ConfigError::AccountConfigParse { .. },
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
    use crate::services::auth::AuthError;

    match err {
        AppError::ChildNotFound { .. } => {
            Some("set CODEX_SESSION_CHILD_BIN or add codex to PATH and retry")
        }
        AppError::ChildNotExecutable { .. } => {
            Some("chmod +x the child binary or point CODEX_SESSION_CHILD_BIN at an executable file")
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
        AppError::Config(ConfigError::ProfileNotFound { .. }) => {
            Some("run `codex-session profile list` to inspect available profiles")
        }
        AppError::Auth(AuthError::SymlinkRefused { .. }) => {
            Some("replace ~/.codex/auth.json with a regular file owned by you (mode 0600)")
        }
        AppError::Auth(AuthError::HardlinkRefused { .. }) => {
            Some("remove the extra hard link so ~/.codex/auth.json has nlink == 1")
        }
        AppError::Auth(AuthError::BadOwnership { .. }) => {
            Some("chown the file to your user and `chmod 0600 ~/.codex/auth.json`")
        }
        AppError::Account(crate::services::account::AccountError::NotFound { .. }) => {
            Some("run `codex-session account list` to inspect registered accounts")
        }
        AppError::Account(crate::services::account::AccountError::AlreadyExists { .. }) => {
            Some("choose a different account name or remove the existing account first")
        }
        AppError::Config(
            ConfigError::NoXdg
            | ConfigError::NoHomeDir
            | ConfigError::CurrentDir(_)
            | ConfigError::Parse { .. }
            | ConfigError::NonUtf8Path(_)
            | ConfigError::Io(_)
            | ConfigError::EnvParse { .. }
            | ConfigError::ManifestParse { .. }
            | ConfigError::ManifestSchema { .. }
            | ConfigError::LayerNotFound { .. }
            | ConfigError::LayerParse { .. }
            | ConfigError::MergeFailed { .. }
            | ConfigError::SessionDirUnresolvable { .. }
            | ConfigError::EnvKeyInvalid { .. }
            | ConfigError::AccountConfigParse { .. },
        )
        | AppError::Auth(
            AuthError::Io { .. } | AuthError::LockFailed { .. } | AuthError::JsonParse { .. },
        )
        | AppError::Account(
            crate::services::account::AccountError::InvalidName { .. }
            | crate::services::account::AccountError::RegistryIo { .. }
            | crate::services::account::AccountError::NoEligible
            | crate::services::account::AccountError::QuotaFetch { .. }
            | crate::services::account::AccountError::QuotaParse { .. },
        )
        | AppError::Usage(_)
        | AppError::ChildExec(_)
        | AppError::ChildExitNonZero(_)
        | AppError::ChildSignaled(_)
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
    fn other_is_seventy() {
        let err = AppError::Other(anyhow::anyhow!("boom"));
        assert_eq!(err.exit_code(), 70);
        assert_eq!(err.kind(), "internal");
    }

    #[test]
    fn auth_symlink_refused_is_seventy_five() {
        let err = AppError::Auth(crate::services::auth::AuthError::SymlinkRefused {
            path: camino::Utf8PathBuf::from("/home/u/.codex/auth.json"),
        });
        assert_eq!(err.exit_code(), 75);
        assert_eq!(err.kind(), "auth-symlink-refused");
    }

    #[test]
    fn auth_hardlink_refused_is_seventy_five() {
        let err = AppError::Auth(crate::services::auth::AuthError::HardlinkRefused {
            path: camino::Utf8PathBuf::from("/home/u/.codex/auth.json"),
        });
        assert_eq!(err.exit_code(), 75);
        assert_eq!(err.kind(), "auth-hardlink-refused");
    }

    #[test]
    fn auth_bad_ownership_is_seventy_five() {
        let err = AppError::Auth(crate::services::auth::AuthError::BadOwnership {
            path: camino::Utf8PathBuf::from("/home/u/.codex/auth.json"),
        });
        assert_eq!(err.exit_code(), 75);
        assert_eq!(err.kind(), "auth-bad-ownership");
    }

    #[test]
    fn account_invalid_name_maps_to_usage() {
        let err = AppError::Account(crate::services::account::AccountError::InvalidName {
            value: "BAD".to_owned(),
            reason: "must start with a lowercase ASCII letter or digit".to_owned(),
        });
        assert_eq!(err.exit_code(), 64);
        assert_eq!(err.kind(), "account-invalid-name");
    }

    #[test]
    fn account_not_found_maps_to_config() {
        let err = AppError::Account(crate::services::account::AccountError::NotFound {
            name: crate::services::account::AccountId::from_unchecked("work".to_owned()),
            path: camino::Utf8PathBuf::from("/tmp/accounts/work"),
        });
        assert_eq!(err.exit_code(), 78);
        assert_eq!(err.kind(), "account-not-found");
    }

    #[test]
    fn account_already_exists_maps_to_config() {
        let err = AppError::Account(crate::services::account::AccountError::AlreadyExists {
            name: crate::services::account::AccountId::from_unchecked("work".to_owned()),
            path: camino::Utf8PathBuf::from("/tmp/accounts/work"),
        });
        assert_eq!(err.exit_code(), 78);
        assert_eq!(err.kind(), "account-already-exists");
    }

    #[test]
    fn account_registry_io_maps_to_ioerr() {
        let err = AppError::Account(crate::services::account::AccountError::RegistryIo {
            path: camino::Utf8PathBuf::from("/tmp/accounts"),
            source: std::io::Error::from(std::io::ErrorKind::Other),
        });
        assert_eq!(err.exit_code(), 74);
        assert_eq!(err.kind(), "account-registry-io");
    }

    #[test]
    fn account_no_eligible_maps_to_tempfail() {
        let err = AppError::Account(crate::services::account::AccountError::NoEligible);
        assert_eq!(err.exit_code(), 75);
        assert_eq!(err.kind(), "account-no-eligible");
    }

    #[test]
    fn account_quota_fetch_maps_to_unavailable() {
        let err = AppError::Account(crate::services::account::AccountError::QuotaFetch {
            detail: "boom".to_owned(),
        });
        assert_eq!(err.exit_code(), 69);
        assert_eq!(err.kind(), "account-quota-fetch");
    }

    #[test]
    fn account_quota_parse_maps_to_dataerr() {
        let err = AppError::Account(crate::services::account::AccountError::QuotaParse {
            detail: "boom".to_owned(),
        });
        assert_eq!(err.exit_code(), 65);
        assert_eq!(err.kind(), "account-quota-parse");
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
