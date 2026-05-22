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
    /// Archive an account to accounts/.trash/.
    Remove(AccountRemoveArgs),
}

#[derive(Debug, Clone, clap::Args)]
pub(crate) struct AccountAddArgs {
    /// Account name (regex `[a-z0-9][a-z0-9_-]{0,31}`).
    pub(crate) name: AccountId,
    /// Seed ~/.codex/auth.json into the new account's seed file.
    #[arg(long)]
    pub(crate) from_native: bool,
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
}

#[derive(Debug, Clone, clap::Args)]
pub(crate) struct AccountRemoveArgs {
    /// Account name to archive.
    pub(crate) name: AccountId,
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
