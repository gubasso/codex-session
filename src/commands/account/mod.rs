#![allow(clippy::missing_errors_doc, clippy::result_large_err)]

pub(crate) mod add;
pub(crate) mod cooldown;
pub(crate) mod current;
pub(crate) mod list;
pub(crate) mod quota;
pub(crate) mod remove;
pub(crate) mod use_;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct AccountListView {
    pub(crate) active: Option<AccountCurrentView>,
    pub(crate) accounts: Vec<AccountListEntryView>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct AccountListEntryView {
    pub(crate) name: String,
    pub(crate) dir: camino::Utf8PathBuf,
    pub(crate) has_auth: bool,
    pub(crate) last_used_at_unix: Option<u64>,
    pub(crate) current: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct AccountCurrentView {
    pub(crate) name: String,
    pub(crate) source: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct AccountMutationView {
    pub(crate) name: String,
    pub(crate) path: camino::Utf8PathBuf,
    pub(crate) archived_to: Option<camino::Utf8PathBuf>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct AccountQuotaWindowView {
    pub(crate) percent_left: f64,
    pub(crate) reset_at_unix: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct AccountQuotaEntryView {
    pub(crate) account: String,
    pub(crate) active: bool,
    pub(crate) mode: String,
    pub(crate) fetched_at_unix: u64,
    pub(crate) ttl_secs: u64,
    pub(crate) stale: bool,
    pub(crate) live: bool,
    pub(crate) error: Option<String>,
    pub(crate) five_hour: Option<AccountQuotaWindowView>,
    pub(crate) weekly: Option<AccountQuotaWindowView>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct AccountCooldownView {
    pub(crate) entries: Vec<AccountCooldownEntryView>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct AccountCooldownEntryView {
    pub(crate) account: String,
    pub(crate) cooled_down: bool,
    pub(crate) reset_at_unix: Option<u64>,
    pub(crate) reset_in_seconds: Option<u64>,
    pub(crate) reason: Option<String>,
    pub(crate) last_429_at_unix: Option<u64>,
}

pub(crate) fn dispatch(
    ctx: &crate::context::AppContext,
    args: crate::cli::account::AccountArgs,
) -> Result<(), crate::error::AppError> {
    use crate::cli::account::AccountCommand;

    match args.command {
        AccountCommand::Add(args) => add::run(ctx, &args),
        AccountCommand::List(args) => list::run(ctx, args),
        AccountCommand::Current(args) => current::run(ctx, args),
        AccountCommand::Use(args) => use_::run(ctx, &args),
        AccountCommand::Remove(args) => remove::run(ctx, &args),
        AccountCommand::Quota(args) => quota::run(ctx, args),
        AccountCommand::Cooldown(args) => cooldown::run(ctx, args),
    }
}

fn as_unix(ts: Option<std::time::SystemTime>) -> Option<u64> {
    ts.and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|value| value.as_secs())
}
