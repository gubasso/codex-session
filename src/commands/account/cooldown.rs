//! `account cooldown` command.
#![allow(clippy::missing_errors_doc, clippy::result_large_err)]

use crate::cli::account::{
    AccountCooldownArgs, AccountCooldownClearArgs, AccountCooldownCommand, AccountCooldownShowArgs,
    AccountSelector,
};
use crate::commands::account::{AccountCooldownEntryView, AccountCooldownView};
use crate::services::account::{AccountError, AccountId, cooldown, registry::Registry};

pub(crate) fn run(
    ctx: &crate::context::AppContext,
    args: AccountCooldownArgs,
) -> Result<(), crate::error::AppError> {
    match args
        .command
        .unwrap_or(AccountCooldownCommand::Show(AccountCooldownShowArgs {
            json: false,
        })) {
        AccountCooldownCommand::Show(args) => show(ctx, &args),
        AccountCooldownCommand::Clear(args) => clear(ctx, &args),
    }
}

fn show(
    ctx: &crate::context::AppContext,
    args: &AccountCooldownShowArgs,
) -> Result<(), crate::error::AppError> {
    let registry = Registry::from_config(&ctx.config);
    let entries = if let Some(account) = selected_account(ctx)? {
        // Reject unknown accounts up front; otherwise we'd fabricate an
        // "eligible" row for a name that has never been registered.
        registry.expect_account_dir(account)?;
        vec![entry_for(&registry, account)?]
    } else {
        let mut entries = Vec::new();
        for account in registry.list()? {
            entries.push(entry_for(&registry, &account.id)?);
        }
        entries
    };
    ctx.ui.write_account_cooldowns(
        &AccountCooldownView { entries },
        if args.json {
            crate::cli::OutputFormat::Json
        } else {
            crate::cli::OutputFormat::Text
        },
    )?;
    Ok(())
}

fn clear(
    ctx: &crate::context::AppContext,
    args: &AccountCooldownClearArgs,
) -> Result<(), crate::error::AppError> {
    // Check --all/--account conflict first so it fires for both Auto and Named
    // selectors with the same diagnostic. Calling selected_account() before
    // this check would short-circuit on --account auto with a misleading
    // "auto is not supported" error.
    if args.all && ctx.global.account.is_some() {
        return Err(crate::error::AppError::Usage(clap::Error::raw(
            clap::error::ErrorKind::ArgumentConflict,
            "--all conflicts with --account; pick one",
        )));
    }

    let registry = Registry::from_config(&ctx.config);
    if args.all {
        let cleared = cooldown::clear_all(&registry).map_err(AccountError::from)?;
        tracing::info!(op = "cooldown.clear", account = "all", cleared);
        return Ok(());
    }

    let selected = selected_account(ctx)?;
    let Some(account) = selected else {
        return Err(crate::error::AppError::Usage(clap::Error::raw(
            clap::error::ErrorKind::MissingRequiredArgument,
            "either --account <NAME> or --all is required",
        )));
    };
    // Reject unknown accounts up front with a real `AccountError::NotFound`
    // instead of silently no-op'ing a `cooldown.json` removal under a
    // fabricated path.
    let account_root = registry.expect_account_dir(account)?;
    cooldown::clear(&account_root).map_err(AccountError::from)?;
    tracing::info!(op = "cooldown.clear", account = %account);
    Ok(())
}

fn entry_for(
    registry: &Registry,
    account: &AccountId,
) -> Result<AccountCooldownEntryView, crate::error::AppError> {
    let account_root = registry.account_dir(account);
    let state = cooldown::read(&account_root).map_err(AccountError::from)?;
    let now_unix = now_unix();
    Ok(match state {
        Some(cd) => {
            let cooled_down = cooldown::is_active(&cd, now_unix);
            AccountCooldownEntryView {
                account: account.to_string(),
                cooled_down,
                reset_at_unix: Some(cd.reset_at_unix),
                reset_in_seconds: cooled_down.then_some(cd.reset_at_unix.saturating_sub(now_unix)),
                reason: Some(cd.reason),
                last_429_at_unix: Some(cd.last_429_at_unix),
            }
        }
        None => AccountCooldownEntryView {
            account: account.to_string(),
            cooled_down: false,
            reset_at_unix: None,
            reset_in_seconds: None,
            reason: None,
            last_429_at_unix: None,
        },
    })
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

fn selected_account(
    ctx: &crate::context::AppContext,
) -> Result<Option<&AccountId>, crate::error::AppError> {
    match ctx.global.account.as_ref() {
        Some(AccountSelector::Named(account)) => Ok(Some(account)),
        Some(AccountSelector::Auto) => Err(crate::error::AppError::Usage(clap::Error::raw(
            clap::error::ErrorKind::InvalidValue,
            "--account auto is not supported for account cooldown; pass a concrete account name",
        ))),
        None => Ok(None),
    }
}
