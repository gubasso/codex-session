use camino::Utf8PathBuf;

use super::{cooldown::CooldownError, id::AccountId};

#[derive(Debug, Clone, Copy, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OutcomeState {
    FiveHourExhausted,
    WeeklyExhausted,
    RateLimited429,
    AuthFailed401,
    Cooldown,
    BelowKnee,
    NoAuth,
    TokenExpired,
    NotAttempted,
    AttemptedUnknown,
}

impl OutcomeState {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::FiveHourExhausted => "five_hour_exhausted",
            Self::WeeklyExhausted => "weekly_exhausted",
            Self::RateLimited429 => "rate_limited_429",
            Self::AuthFailed401 => "auth_failed_401",
            Self::Cooldown => "cooldown",
            Self::BelowKnee => "below_knee",
            Self::NoAuth => "no_auth",
            Self::TokenExpired => "token_expired",
            Self::NotAttempted => "not_attempted",
            Self::AttemptedUnknown => "attempted_unknown",
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct AccountOutcomeLine {
    pub(crate) id: AccountId,
    pub(crate) outcome: String,
    pub(crate) state: OutcomeState,
    pub(crate) five_hour_left: Option<f64>,
    pub(crate) weekly_left: Option<f64>,
    pub(crate) available_at_unix: Option<u64>,
}

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
    NoEligible { report: Vec<AccountOutcomeLine> },

    #[error("quota fetch failed: {detail}")]
    QuotaFetch { detail: String },

    #[error("quota parse failed: {detail}")]
    QuotaParse { detail: String },

    #[error("requires an interactive terminal for {action}")]
    NonInteractive { action: String },

    #[error("codex login failed: {detail}")]
    LoginFailed { detail: String },

    #[error("no native auth found at ~/.codex/auth.json; run `codex login` first")]
    NativeAuthMissing,

    #[error("auto-selection exhausted; no account could complete the request")]
    AutoExhausted { report: Vec<AccountOutcomeLine> },

    #[error("resume blocked; account owner cannot continue thread {thread_id}")]
    ResumeBlocked {
        thread_id: String,
        owner: AccountOutcomeLine,
        others: Vec<AccountOutcomeLine>,
    },

    #[error(
        "account `{name}` has no valid authentication; run `codex-session account refresh {name}`"
    )]
    AuthMissing { name: AccountId },

    #[error("no accounts registered; run `codex-session account add <name>`")]
    NoAccounts,

    #[error(
        "accounts exist but none is selected; \
        pass --account <name> to pin one, or run codex-session login"
    )]
    NoneSelected,

    #[error("health probe requires profiles/ping.config.toml: {detail}")]
    PingProfileMissing { detail: String },

    #[error(transparent)]
    Cooldown(#[from] CooldownError),
}

impl AccountError {
    pub(crate) const fn kind(&self) -> &'static str {
        match self {
            Self::InvalidName { .. } => "account-invalid-name",
            Self::NotFound { .. } => "account-not-found",
            Self::AlreadyExists { .. } => "account-already-exists",
            Self::RegistryIo { .. } => "account-registry-io",
            Self::NoEligible { .. } => "account-no-eligible",
            Self::QuotaFetch { .. } => "account-quota-fetch",
            Self::QuotaParse { .. } => "account-quota-parse",
            Self::NonInteractive { .. } => "account-non-interactive",
            Self::LoginFailed { .. } => "account-login-failed",
            Self::NativeAuthMissing => "account-native-auth-missing",
            Self::AutoExhausted { .. } => "account-auto-exhausted",
            Self::ResumeBlocked { .. } => "account-resume-blocked",
            Self::AuthMissing { .. } => "account-auth-missing",
            Self::NoAccounts => "account-no-accounts",
            Self::NoneSelected => "account-none-selected",
            Self::PingProfileMissing { .. } => "account-ping-profile-missing",
            Self::Cooldown { .. } => "account-cooldown",
        }
    }

    pub(crate) fn path(&self) -> Option<&camino::Utf8Path> {
        match self {
            Self::NotFound { path, .. }
            | Self::AlreadyExists { path, .. }
            | Self::RegistryIo { path, .. } => Some(path.as_path()),
            Self::Cooldown(err) => match err {
                CooldownError::Fs(
                    crate::adapters::fs::FsError::Io { path, .. }
                    | crate::adapters::fs::FsError::SymlinkRefused { path }
                    | crate::adapters::fs::FsError::HardlinkRefused { path }
                    | crate::adapters::fs::FsError::BadOwnership { path, .. },
                )
                | CooldownError::Decode { path, .. }
                | CooldownError::Encode { path, .. }
                | CooldownError::Io { path, .. } => Some(path.as_path()),
            },
            Self::InvalidName { .. }
            | Self::NoEligible { .. }
            | Self::QuotaFetch { .. }
            | Self::QuotaParse { .. }
            | Self::NonInteractive { .. }
            | Self::LoginFailed { .. }
            | Self::NativeAuthMissing
            | Self::AutoExhausted { .. }
            | Self::ResumeBlocked { .. }
            | Self::AuthMissing { .. }
            | Self::NoAccounts
            | Self::NoneSelected
            | Self::PingProfileMissing { .. } => None,
        }
    }
}
