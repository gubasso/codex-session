//! Config-layer error type.
//!
//! What this is: typed failures produced while resolving layered wrapper
//! configuration.
//! What this is not: top-level exit-code mapping; that lives in `error.rs`.

/// Stable machine-readable config error keys.
///
/// These keys are emitted in structured logs via `AppError::kind()`.
impl ConfigError {
    /// Return the stable machine-readable kind for this config error.
    pub(crate) const fn kind(&self) -> &'static str {
        match self {
            Self::NoXdg => "config-no-xdg",
            Self::CurrentDir(_) => "config-current-dir",
            Self::Parse { .. } => "config-parse",
            Self::UnknownKey { .. } => "config-unknown-key",
            Self::ExplicitConfigMissing(_) => "config-explicit-missing",
            Self::NonUtf8Path(_) => "config-non-utf8-path",
            Self::Io(_) => "config-io",
            Self::EnvParse { .. } => "config-env-parse",
        }
    }
}

/// Configuration loading failures.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ConfigError {
    /// No usable XDG directories could be resolved.
    #[error("config: missing XDG directories (no HOME?)")]
    NoXdg,

    /// The current working directory could not be read while searching for a project config.
    #[error("config: failed to read current working directory")]
    CurrentDir(#[source] std::io::Error),

    /// A TOML or extracted config value was invalid.
    #[error("config: parse error in {path}")]
    Parse {
        /// Source file path when one is known.
        path: camino::Utf8PathBuf,
        /// Underlying figment parse failure with provider provenance.
        #[source]
        source: figment::Error,
    },

    /// A config file contained an unknown key.
    #[error("config: unknown key {key} in {path}")]
    UnknownKey {
        /// Unknown key name.
        key: String,
        /// Source file path.
        path: camino::Utf8PathBuf,
    },

    /// The explicit `--config` file was missing.
    #[error("config: explicit config file does not exist: {0}")]
    ExplicitConfigMissing(camino::Utf8PathBuf),

    /// A required path was not valid UTF-8.
    #[error("config: non-utf8 path")]
    NonUtf8Path(#[from] camino::FromPathBufError),

    /// A supporting I/O operation failed.
    #[error("config: io error")]
    Io(#[from] std::io::Error),

    /// An environment override could not be parsed.
    #[error("config: invalid environment override {key}={value} (expected {expected})")]
    EnvParse {
        key: String,
        value: String,
        expected: &'static str,
    },
}

#[derive(Debug)]
pub(crate) struct EnvParseError {
    /// Environment variable key that failed to parse.
    pub(crate) key: String,
    /// Raw environment variable value.
    pub(crate) value: String,
    /// Human-readable expectation for the accepted value shape.
    pub(crate) expected: &'static str,
}

impl From<EnvParseError> for ConfigError {
    fn from(value: EnvParseError) -> Self {
        Self::EnvParse {
            key: value.key,
            value: value.value,
            expected: value.expected,
        }
    }
}
