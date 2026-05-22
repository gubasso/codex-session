use camino::Utf8PathBuf;

use super::id::AccountId;

#[allow(dead_code)]
#[derive(Debug, thiserror::Error)]
pub(crate) enum AccountError {
    #[error("invalid account name `{value}`: {reason}")]
    InvalidName { value: String, reason: String },

    #[error("account `{name}` not found at {path}")]
    NotFound { name: AccountId, path: Utf8PathBuf },

    #[error("account `{name}` already exists at {path}")]
    AlreadyExists { name: AccountId, path: Utf8PathBuf },

    #[error("registry io at {path}: {source}")]
    RegistryIo {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("no eligible account")]
    NoEligible,

    #[error("quota fetch failed: {detail}")]
    QuotaFetch { detail: String },

    #[error("quota parse failed: {detail}")]
    QuotaParse { detail: String },
}

impl AccountError {
    pub(crate) const fn kind(&self) -> &'static str {
        match self {
            Self::InvalidName { .. } => "account-invalid-name",
            Self::NotFound { .. } => "account-not-found",
            Self::AlreadyExists { .. } => "account-already-exists",
            Self::RegistryIo { .. } => "account-registry-io",
            Self::NoEligible => "account-no-eligible",
            Self::QuotaFetch { .. } => "account-quota-fetch",
            Self::QuotaParse { .. } => "account-quota-parse",
        }
    }

    pub(crate) fn path(&self) -> Option<&camino::Utf8Path> {
        match self {
            Self::NotFound { path, .. }
            | Self::AlreadyExists { path, .. }
            | Self::RegistryIo { path, .. } => Some(path.as_path()),
            Self::InvalidName { .. }
            | Self::NoEligible
            | Self::QuotaFetch { .. }
            | Self::QuotaParse { .. } => None,
        }
    }
}
