//! `account` subcommand: parse-shape.

use crate::services::account::AccountId;

#[derive(Debug, clap::Args)]
pub(crate) struct AccountArgs {
    #[command(subcommand)]
    pub(crate) command: AccountCommand,
}

#[derive(Debug, clap::Subcommand)]
pub(crate) enum AccountCommand {
    /// Register a new account.
    Add(AccountAddArgs),
    /// List registered accounts.
    List(AccountListArgs),
    /// Print the currently active account and its source.
    Current(AccountCurrentArgs),
    /// Pin the active account (writes state/last-account).
    Use(AccountUseArgs),
    /// Remove an account permanently.
    Remove(AccountRemoveArgs),
    /// Refresh an account's root auth.json from the native codex login.
    Refresh(AccountRefreshArgs),
    /// Read per-account quota from the live wham/usage endpoint.
    Quota(AccountQuotaArgs),
    /// Check account token health, quota score, and probe status.
    Health(AccountHealthArgs),
    /// Show or clear failover cooldown state.
    Cooldown(AccountCooldownArgs),
}

#[derive(Debug, Clone, clap::Args)]
pub(crate) struct AccountAddArgs {
    /// Account name (regex `[a-z0-9][a-z0-9_-]{0,31}`).
    pub(crate) name: AccountId,
    /// Output format for the mutation result.
    #[arg(long, value_name = "FMT", value_enum, default_value_t = crate::cli::OutputFormat::Text)]
    pub(crate) format: crate::cli::OutputFormat,
}

#[derive(Debug, Clone, Copy, clap::Args)]
pub(crate) struct AccountListArgs {
    /// Output format for the reported account list.
    #[arg(long, value_enum, default_value_t = crate::cli::OutputFormat::Text)]
    pub(crate) format: crate::cli::OutputFormat,
}

#[derive(Debug, Clone, Copy, clap::Args)]
pub(crate) struct AccountCurrentArgs {
    /// Output format for the reported current account.
    #[arg(long, value_enum, default_value_t = crate::cli::OutputFormat::Text)]
    pub(crate) format: crate::cli::OutputFormat,
}

#[derive(Debug, Clone, clap::Args)]
pub(crate) struct AccountUseArgs {
    /// Account name to pin.
    pub(crate) name: AccountId,
    /// Output format for the mutation result.
    #[arg(long, value_name = "FMT", value_enum, default_value_t = crate::cli::OutputFormat::Text)]
    pub(crate) format: crate::cli::OutputFormat,
}

#[derive(Debug, Clone, clap::Args)]
pub(crate) struct AccountRemoveArgs {
    /// Account name to remove permanently.
    pub(crate) name: AccountId,
    /// Skip interactive confirmation.
    #[arg(long)]
    pub(crate) yes: bool,
    /// Output format for the mutation result.
    #[arg(long, value_name = "FMT", value_enum, default_value_t = crate::cli::OutputFormat::Text)]
    pub(crate) format: crate::cli::OutputFormat,
}

#[derive(Debug, Clone, clap::Args)]
pub(crate) struct AccountRefreshArgs {
    /// Account name to refresh. Defaults to the current account if omitted.
    pub(crate) name: Option<AccountId>,
    /// Output format for the mutation result.
    #[arg(long, value_name = "FMT", value_enum, default_value_t = crate::cli::OutputFormat::Text)]
    pub(crate) format: crate::cli::OutputFormat,
}

#[derive(Debug, Clone, Copy, clap::Args)]
pub(crate) struct AccountQuotaArgs {
    /// Deprecated: all accounts are shown by default. Accepted but ignored.
    #[arg(long, hide = true)]
    pub(crate) all: bool,
    /// Output format for the reported quota.
    #[arg(long, value_enum, default_value_t = crate::cli::OutputFormat::Text)]
    pub(crate) format: crate::cli::OutputFormat,
    /// Include detailed scoring fields in text mode.
    #[arg(long)]
    pub(crate) detail: bool,
}

#[derive(Debug, Clone, Copy, clap::Args)]
pub(crate) struct AccountHealthArgs {
    /// Output format for account health.
    #[arg(long, value_enum, default_value_t = crate::cli::OutputFormat::Text)]
    pub(crate) format: crate::cli::OutputFormat,
    /// Include detailed scoring fields in text mode.
    #[arg(long)]
    pub(crate) detail: bool,
    /// Local-only mode: do not refresh or probe; accept stale cache.
    #[arg(long)]
    pub(crate) fast: bool,
}

#[derive(Debug, clap::Args)]
pub(crate) struct AccountCooldownArgs {
    #[command(subcommand)]
    pub(crate) command: Option<AccountCooldownCommand>,
}

#[derive(Debug, clap::Subcommand)]
pub(crate) enum AccountCooldownCommand {
    /// Show current cooldown state for one or all accounts.
    Show(AccountCooldownShowArgs),
    /// Clear cooldown for one account or all of them.
    Clear(AccountCooldownClearArgs),
}

#[derive(Debug, Clone, Copy, clap::Args)]
pub(crate) struct AccountCooldownShowArgs {
    /// Output format for cooldown state.
    #[arg(long, value_name = "FMT", value_enum, default_value_t = crate::cli::OutputFormat::Text)]
    pub(crate) format: crate::cli::OutputFormat,
}

#[derive(Debug, Clone, clap::Args)]
pub(crate) struct AccountCooldownClearArgs {
    /// Clear cooldowns for every registered account; conflicts with --account.
    #[arg(long)]
    pub(crate) all: bool,
}

/// Selector for the `--account` global flag: a name or the literal `auto`.
#[derive(Debug, Clone)]
pub(crate) enum AccountSelector {
    Named(AccountId),
    Auto,
}

impl std::str::FromStr for AccountSelector {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value == "auto" {
            return Ok(Self::Auto);
        }
        AccountId::from_str(value).map(Self::Named)
    }
}
