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
            Self::NoHomeDir => "config-no-home-dir",
            Self::CurrentDir(_) => "config-current-dir",
            Self::Parse { .. } => "config-parse",
            Self::UnknownKey { .. } => "config-unknown-key",
            Self::ExplicitConfigMissing(_) => "config-explicit-missing",
            Self::NonUtf8Path(_) => "config-non-utf8-path",
            Self::Io(_) => "config-io",
            Self::EnvParse { .. } => "config-env-parse",
            Self::ProfileNotFound { .. } => "profile-not-found",
            Self::ManifestParse { .. } => "manifest-parse",
            Self::ManifestSchema { .. } => "manifest-schema",
            Self::LayerNotFound { .. } => "layer-not-found",
            Self::LayerParse { .. } => "layer-parse",
            Self::MergeFailed { .. } => "merge-failed",
            Self::SessionDirUnresolvable { .. } => "session-dir-unresolvable",
            Self::EnvKeyInvalid { .. } => "env-key-invalid",
            Self::AccountConfigParse { .. } => "config-account-parse",
        }
    }
}

/// Configuration loading failures.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ConfigError {
    /// No usable XDG directories could be resolved.
    #[error("config: missing XDG directories (no HOME?)")]
    NoXdg,

    /// The home directory could not be resolved.
    #[error("config: home directory could not be resolved")]
    NoHomeDir,

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

    /// A named profile manifest was not found.
    #[error("config: profile `{name}` not found")]
    ProfileNotFound {
        name: String,
        path: camino::Utf8PathBuf,
    },

    /// A profile manifest could not be parsed as YAML.
    #[error("config: failed to parse profile manifest {path}")]
    ManifestParse {
        path: camino::Utf8PathBuf,
        #[source]
        source: serde_yaml_ng::Error,
    },

    /// A profile manifest was syntactically valid YAML but failed schema validation.
    #[error("config: invalid profile manifest {path}")]
    ManifestSchema {
        path: camino::Utf8PathBuf,
        reason: String,
    },

    /// A referenced layer file did not exist.
    #[error("config: settings layer `{name}` not found at {path}")]
    LayerNotFound {
        name: String,
        path: camino::Utf8PathBuf,
    },

    /// A referenced layer file failed TOML parsing.
    #[error("config: failed to parse settings layer {path}")]
    LayerParse {
        path: camino::Utf8PathBuf,
        #[source]
        source: toml::de::Error,
    },

    /// Reserved merge failure used by the new composition pipeline.
    #[error("config: composition merge failed")]
    MergeFailed { reason: String },

    /// No secure session directory root could be resolved.
    #[error("config: secure session directory could not be resolved")]
    SessionDirUnresolvable {
        runtime: Option<camino::Utf8PathBuf>,
        state: Option<camino::Utf8PathBuf>,
        reason: String,
    },

    /// An invalid key/value pair appeared in a profile `[env]` table.
    #[error("config: invalid profile env key `{key}`")]
    EnvKeyInvalid { key: String, reason: String },

    /// An invalid account config value could not be parsed into `AccountId`.
    #[error("config: invalid `{field}` value `{value}`: {reason}")]
    AccountConfigParse {
        field: &'static str,
        value: String,
        reason: String,
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
