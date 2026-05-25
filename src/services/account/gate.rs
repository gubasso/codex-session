//! Auth gate — ensures an account is resolved and authenticated before launch.
//!
//! Also provides `run_login` / `run_logout` for `codex-session login` and
//! `codex-session logout` — these are managed by the gate (not raw
//! pass-throughs) so that every auth operation is account-aware and narrated.
//!
//! Auth validity is determined by seed-file existence only. There is no
//! server-side token probe; see `docs/auth-gate-spec.md` §2.3 for rationale.
#![allow(clippy::result_large_err)]

use std::io::IsTerminal as _;

use super::{
    AccountError, AccountId,
    registry::{AccountEntry, Registry},
    resolver::{AccountResolutionSource, ResolvedAccount, source_label},
};

use crate::context::AppContext;
use crate::error::AppError;

#[derive(Debug)]
pub(crate) enum AccountState {
    Ready(ResolvedAccount),
    AuthMissing { account: AccountId },
    NoneSelected { accounts: Vec<AccountEntry> },
    NoAccounts,
}

pub(crate) fn assess(ctx: &AppContext) -> Result<AccountState, AppError> {
    let registry = Registry::from_config(&ctx.config);
    let accounts = registry.list()?;

    if accounts.is_empty() {
        return Ok(AccountState::NoAccounts);
    }

    match super::resolver::resolve(ctx) {
        Err(AppError::Account(AccountError::NoneResolved)) => {
            Ok(AccountState::NoneSelected { accounts })
        }
        Err(err) => Err(err),
        Ok(resolved) => match registry.expect_account_dir(&resolved.id) {
            Err(AccountError::NotFound { name, .. }) => {
                tracing::warn!(
                    op = "gate.assess",
                    outcome = "stale-pointer",
                    account = %name,
                    "resolved account directory missing; treating as none-selected"
                );
                Ok(AccountState::NoneSelected { accounts })
            }
            Err(err) => Err(err.into()),
            Ok(_) => {
                let seed_path = registry.group_auth_seed_path(&resolved.id);
                if !seed_path.as_std_path().exists() {
                    return Ok(AccountState::AuthMissing {
                        account: resolved.id,
                    });
                }
                Ok(AccountState::Ready(resolved))
            }
        },
    }
}

pub(crate) fn ensure(ctx: &AppContext) -> Result<ResolvedAccount, AppError> {
    let state = assess(ctx)?;
    let interactive = std::io::stdin().is_terminal();

    match state {
        AccountState::Ready(resolved) => {
            narrate(
                ctx,
                &format!(
                    "account '{}' (source: {}), auth present — launching.",
                    resolved.id,
                    source_label(resolved.source),
                ),
            );
            Ok(resolved)
        }
        AccountState::NoAccounts => {
            if !interactive {
                return Err(AccountError::NoAccounts.into());
            }
            narrate(
                ctx,
                "no accounts registered. Let's set up your first account.",
            );
            interactive_resolve_no_accounts(ctx)
        }
        AccountState::NoneSelected { accounts } => {
            if !interactive {
                return Err(AccountError::NoneSelected.into());
            }
            narrate(
                ctx,
                &format!(
                    "{} account(s) found but none is selected. Please choose one.",
                    accounts.len(),
                ),
            );
            interactive_resolve_none_selected(ctx, &accounts)
        }
        AccountState::AuthMissing { account, .. } => {
            if !interactive {
                return Err(AccountError::AuthMissing { name: account }.into());
            }
            warn_and_confirm_auth_missing(ctx, &account)
        }
    }
}

fn warn_and_confirm_auth_missing(
    ctx: &AppContext,
    account: &AccountId,
) -> Result<ResolvedAccount, AppError> {
    let _ = ctx.ui.write_warning(&format!(
        "\nwarning: account '{account}' is selected but has no valid authentication token.\n\
        The token may be missing or expired. You need to re-authenticate before launching.\n",
    ));
    interactive_resolve_auth_missing(ctx, account)
}

fn interactive_resolve_no_accounts(ctx: &AppContext) -> Result<ResolvedAccount, AppError> {
    let account_id = prompt_account_name()?;
    narrate(
        ctx,
        &format!("creating account '{account_id}' and starting authentication..."),
    );
    do_add_account(ctx, &account_id)?;
    narrate(
        ctx,
        &format!("account '{account_id}' created and authenticated — launching."),
    );
    Ok(ResolvedAccount {
        id: account_id,
        source: AccountResolutionSource::Interactive,
    })
}

fn interactive_resolve_none_selected(
    ctx: &AppContext,
    accounts: &[AccountEntry],
) -> Result<ResolvedAccount, AppError> {
    let selected = prompt_select_account(accounts)?;
    match selected {
        AccountSelection::Existing(account_id) => {
            let has_auth = accounts
                .iter()
                .find(|a| a.id == account_id)
                .is_some_and(|a| a.has_auth);
            if !has_auth {
                narrate(
                    ctx,
                    &format!("account '{account_id}' has no authentication token."),
                );
                return interactive_resolve_auth_missing(ctx, &account_id);
            }
            let registry = Registry::from_config(&ctx.config);
            registry.set_current(&account_id)?;
            narrate(
                ctx,
                &format!("account '{account_id}' selected — launching."),
            );
            Ok(ResolvedAccount {
                id: account_id,
                source: AccountResolutionSource::Interactive,
            })
        }
        AccountSelection::AddNew => {
            let account_id = prompt_account_name()?;
            narrate(
                ctx,
                &format!("creating account '{account_id}' and starting authentication..."),
            );
            do_add_account(ctx, &account_id)?;
            narrate(
                ctx,
                &format!("account '{account_id}' created and authenticated — launching."),
            );
            Ok(ResolvedAccount {
                id: account_id,
                source: AccountResolutionSource::Interactive,
            })
        }
    }
}

fn interactive_resolve_auth_missing(
    ctx: &AppContext,
    account: &AccountId,
) -> Result<ResolvedAccount, AppError> {
    let options = vec![
        format!("Re-authenticate '{account}'"),
        "Switch to a different account".to_owned(),
        "Add a new account".to_owned(),
    ];

    let choice = inquire::Select::new(
        &format!("Account '{account}' has no valid authentication. What would you like to do?"),
        options,
    )
    .prompt()
    .map_err(|err| AccountError::NonInteractive {
        action: format!("auth gate prompt: {err}"),
    })?;

    if choice.starts_with("Re-authenticate") {
        narrate(
            ctx,
            &format!(
                "re-authenticating account '{account}' — \
                running the codex native login flow...",
            ),
        );
        do_refresh_auth(ctx, account)?;
        narrate(
            ctx,
            &format!("account '{account}' re-authenticated — launching."),
        );
        Ok(ResolvedAccount {
            id: account.clone(),
            source: AccountResolutionSource::Interactive,
        })
    } else if choice.starts_with("Switch") {
        let registry = Registry::from_config(&ctx.config);
        let accounts = registry.list()?;
        interactive_resolve_none_selected(ctx, &accounts)
    } else {
        let account_id = prompt_account_name()?;
        narrate(
            ctx,
            &format!("creating account '{account_id}' and starting authentication..."),
        );
        do_add_account(ctx, &account_id)?;
        narrate(
            ctx,
            &format!("account '{account_id}' created and authenticated — launching."),
        );
        Ok(ResolvedAccount {
            id: account_id,
            source: AccountResolutionSource::Interactive,
        })
    }
}

enum AccountSelection {
    Existing(AccountId),
    AddNew,
}

fn prompt_select_account(accounts: &[AccountEntry]) -> Result<AccountSelection, AppError> {
    let mut options: Vec<String> = accounts
        .iter()
        .map(|a| {
            let auth_status = if a.has_auth {
                "authenticated"
            } else {
                "no auth!"
            };
            format!("{} ({auth_status})", a.id)
        })
        .collect();
    options.push("Add a new account".to_owned());

    let choice = inquire::Select::new("Select an account:", options.clone())
        .prompt()
        .map_err(|err| AccountError::NonInteractive {
            action: format!("account selection prompt: {err}"),
        })?;

    if choice == "Add a new account" {
        return Ok(AccountSelection::AddNew);
    }

    let index = options.iter().position(|o| o == &choice).unwrap_or(0);
    Ok(AccountSelection::Existing(accounts[index].id.clone()))
}

fn prompt_account_name() -> Result<AccountId, AppError> {
    let raw = inquire::Text::new("Account name:")
        .with_help_message("lowercase alphanumeric, hyphens, underscores (1-32 chars)")
        .prompt()
        .map_err(|err| AccountError::NonInteractive {
            action: format!("account name prompt: {err}"),
        })?;
    let id = raw
        .trim()
        .parse::<AccountId>()
        .map_err(|reason| AccountError::InvalidName { value: raw, reason })?;
    Ok(id)
}

fn do_add_account(ctx: &AppContext, account_id: &AccountId) -> Result<(), AppError> {
    let registry = Registry::from_config(&ctx.config);
    registry.add(account_id)?;

    narrate(ctx, "logging out of any existing codex session first...");
    let _ = crate::commands::account::spawn_child(ctx, ["logout"]).inspect_err(|err| {
        tracing::warn!(
            op = "gate.add",
            outcome = "logout-failed-non-fatal",
            error = %err
        );
    });

    narrate(
        ctx,
        &format!(
            "starting codex login — please authenticate in the browser \
            to link account '{account_id}'...",
        ),
    );
    if let Err(err) = crate::commands::account::spawn_child(ctx, ["login"]) {
        let _ = registry.remove(account_id).inspect_err(|cleanup_err| {
            tracing::warn!(
                op = "gate.add",
                outcome = "cleanup-failed",
                account = %account_id,
                error = %cleanup_err
            );
        });
        return Err(AccountError::LoginFailed {
            detail: err.to_string(),
        }
        .into());
    }

    narrate(ctx, "login succeeded — saving authentication token...");
    crate::commands::account::copy_native_auth_to_seed(ctx, &registry, account_id)?;
    registry.set_current(account_id)?;
    Ok(())
}

fn do_refresh_auth(ctx: &AppContext, account: &AccountId) -> Result<(), AppError> {
    narrate(ctx, "logging out of any existing codex session first...");
    let _ = crate::commands::account::spawn_child(ctx, ["logout"]).inspect_err(|err| {
        tracing::warn!(
            op = "gate.refresh",
            outcome = "logout-failed-non-fatal",
            error = %err
        );
    });

    narrate(
        ctx,
        &format!(
            "starting codex login — please authenticate in the browser \
            to renew the token for account '{account}'...",
        ),
    );
    if let Err(err) = crate::commands::account::spawn_child(ctx, ["login"]) {
        return Err(AccountError::LoginFailed {
            detail: err.to_string(),
        }
        .into());
    }

    narrate(
        ctx,
        "login succeeded — saving renewed authentication token...",
    );
    let registry = Registry::from_config(&ctx.config);
    crate::commands::account::copy_native_auth_to_seed(ctx, &registry, account)?;
    narrate(
        ctx,
        "clearing stale group auth tokens so new sessions use the fresh token...",
    );
    registry.delete_group_auths(account)?;
    Ok(())
}

pub(crate) fn run_login(ctx: &AppContext) -> Result<i32, AppError> {
    let state = assess(ctx)?;
    let interactive = std::io::stdin().is_terminal();

    match state {
        AccountState::NoAccounts => {
            if !interactive {
                return Err(AccountError::NoAccounts.into());
            }
            narrate(
                ctx,
                "no accounts registered. Setting up your first account for login.",
            );
            let account_id = prompt_account_name()?;
            narrate(
                ctx,
                &format!("creating account '{account_id}' and starting authentication..."),
            );
            do_add_account(ctx, &account_id)?;
            narrate(
                ctx,
                &format!("account '{account_id}' is now authenticated."),
            );
            Ok(0)
        }
        AccountState::NoneSelected { accounts } => {
            if !interactive {
                return Err(AccountError::NoneSelected.into());
            }
            narrate(
                ctx,
                &format!(
                    "{} account(s) found but none is selected. Please choose one to log in.",
                    accounts.len(),
                ),
            );
            login_resolve_none_selected(ctx, &accounts)?;
            Ok(0)
        }
        AccountState::AuthMissing { account } => {
            narrate(ctx, &format!("re-authenticating account '{account}'..."));
            do_refresh_auth(ctx, &account)?;
            narrate(ctx, &format!("account '{account}' is now authenticated."));
            Ok(0)
        }
        AccountState::Ready(resolved) => {
            narrate(
                ctx,
                &format!(
                    "refreshing authentication for account '{}' (source: {})...",
                    resolved.id,
                    source_label(resolved.source),
                ),
            );
            do_refresh_auth(ctx, &resolved.id)?;
            narrate(
                ctx,
                &format!("account '{}' is now authenticated.", resolved.id),
            );
            Ok(0)
        }
    }
}

pub(crate) fn run_logout(ctx: &AppContext) -> Result<i32, AppError> {
    let state = assess(ctx)?;
    let interactive = std::io::stdin().is_terminal();

    match state {
        AccountState::NoAccounts => {
            narrate(ctx, "no accounts registered — nothing to log out.");
            Ok(0)
        }
        AccountState::NoneSelected { accounts } => {
            if !interactive {
                return Err(AccountError::NoneSelected.into());
            }
            narrate(
                ctx,
                &format!(
                    "{} account(s) found but none is selected. Please choose one to log out.",
                    accounts.len(),
                ),
            );
            let account_id = logout_resolve_none_selected(&accounts)?;
            do_logout(ctx, &account_id)?;
            narrate(ctx, &format!("account '{account_id}' is now logged out."));
            Ok(0)
        }
        AccountState::AuthMissing { account } => {
            narrate(
                ctx,
                &format!("account '{account}' has no valid authentication — already logged out."),
            );
            let registry = Registry::from_config(&ctx.config);
            let _ = registry.delete_auth_seed(&account);
            Ok(0)
        }
        AccountState::Ready(resolved) => {
            narrate(
                ctx,
                &format!(
                    "logging out account '{}' (source: {})...",
                    resolved.id,
                    source_label(resolved.source),
                ),
            );
            do_logout(ctx, &resolved.id)?;
            narrate(
                ctx,
                &format!("account '{}' is now logged out.", resolved.id),
            );
            Ok(0)
        }
    }
}

fn login_resolve_none_selected(
    ctx: &AppContext,
    accounts: &[AccountEntry],
) -> Result<AccountId, AppError> {
    let selected = prompt_select_account(accounts)?;
    match selected {
        AccountSelection::Existing(account_id) => {
            let registry = Registry::from_config(&ctx.config);
            registry.set_current(&account_id)?;
            narrate(
                ctx,
                &format!("selected account '{account_id}' — authenticating..."),
            );
            do_refresh_auth(ctx, &account_id)?;
            narrate(
                ctx,
                &format!("account '{account_id}' is now authenticated."),
            );
            Ok(account_id)
        }
        AccountSelection::AddNew => {
            let account_id = prompt_account_name()?;
            narrate(
                ctx,
                &format!("creating account '{account_id}' and starting authentication..."),
            );
            do_add_account(ctx, &account_id)?;
            narrate(
                ctx,
                &format!("account '{account_id}' is now authenticated."),
            );
            Ok(account_id)
        }
    }
}

fn logout_resolve_none_selected(accounts: &[AccountEntry]) -> Result<AccountId, AppError> {
    let options: Vec<String> = accounts
        .iter()
        .map(|a| {
            let auth_status = if a.has_auth {
                "authenticated"
            } else {
                "no auth"
            };
            format!("{} ({auth_status})", a.id)
        })
        .collect();

    let choice = inquire::Select::new("Select an account to log out:", options.clone())
        .prompt()
        .map_err(|err| AccountError::NonInteractive {
            action: format!("logout account selection: {err}"),
        })?;

    let index = options.iter().position(|o| o == &choice).unwrap_or(0);
    Ok(accounts[index].id.clone())
}

fn do_logout(ctx: &AppContext, account: &AccountId) -> Result<(), AppError> {
    narrate(ctx, "running native codex logout...");
    let _ = crate::commands::account::spawn_child(ctx, ["logout"]).inspect_err(|err| {
        tracing::warn!(
            op = "gate.logout",
            outcome = "native-logout-failed-non-fatal",
            account = %account,
            error = %err
        );
    });

    let registry = Registry::from_config(&ctx.config);
    registry.delete_auth_seed(account)?;
    registry.delete_group_auths(account)?;
    Ok(())
}

fn narrate(ctx: &AppContext, msg: &str) {
    if ctx.global.silent {
        return;
    }
    let _ = ctx.ui.write_prompt(&format!("[codex-session] {msg}\n"));
}
