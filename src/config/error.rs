//! Config-layer error type.

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
        /// Underlying TOML parse failure.
        #[source]
        source: toml::de::Error,
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
    pub(crate) key: String,
    pub(crate) value: String,
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
