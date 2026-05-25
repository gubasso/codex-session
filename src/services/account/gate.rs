//! Auth gate — ensures an account is resolved and authenticated before launch.
//!
//! Every decision and action is narrated to stderr so the user always knows
//! which account was selected, why, and what is about to happen.
#![allow(clippy::result_large_err)]

use std::io::IsTerminal as _;
use std::time::Duration;

use super::{
    AccountError, AccountId,
    registry::{AccountEntry, Registry},
    resolver::{AccountResolutionSource, ResolvedAccount, source_label},
};

use crate::context::AppContext;
use crate::error::AppError;

const AUTH_PROBE_TIMEOUT: Duration = Duration::from_secs(3);

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
                if !probe_token(&seed_path) {
                    tracing::warn!(
                        op = "gate.assess",
                        outcome = "token-revoked",
                        account = %resolved.id,
                        "auth seed exists but token was rejected by the server; \
                        treating as auth-missing"
                    );
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

/// Lightweight server-side token validation.
///
/// Reads the account seed, extracts the OAuth access token, and makes a
/// single HTTP request. Returns `false` only when the server explicitly
/// rejects the token (401/403). Network errors, timeouts, non-OAuth auth
/// (API keys), or unparseable files all return `true` (fail-open) so the
/// gate never blocks offline users or unusual auth setups.
fn probe_token(seed_path: &camino::Utf8Path) -> bool {
    let Ok(bytes) = std::fs::read(seed_path.as_std_path()) else {
        return true;
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return true;
    };
    let Some(access_token) = value
        .get("tokens")
        .and_then(|t| t.get("access_token"))
        .and_then(serde_json::Value::as_str)
        .filter(|s| !s.is_empty())
    else {
        return true;
    };

    let probe_url = std::env::var("CODEX_SESSION_AUTH_PROBE_URL")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "https://chatgpt.com/backend-api/me".to_owned());

    let Ok(client) = reqwest::blocking::Client::builder()
        .connect_timeout(AUTH_PROBE_TIMEOUT)
        .timeout(AUTH_PROBE_TIMEOUT)
        .build()
    else {
        return true;
    };

    match client
        .get(&probe_url)
        .header(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {access_token}"),
        )
        .header(reqwest::header::ACCEPT, "application/json")
        .header(reqwest::header::ORIGIN, "https://chatgpt.com")
        .header(reqwest::header::REFERER, "https://chatgpt.com/")
        .header(reqwest::header::USER_AGENT, "Mozilla/5.0")
        .send()
    {
        Ok(resp) => {
            let status = resp.status().as_u16();
            if status == 401 || status == 403 {
                tracing::info!(op = "gate.probe", status, "server rejected auth token");
                return false;
            }
            true
        }
        Err(err) => {
            tracing::debug!(
                op = "gate.probe", error = %err, "probe request failed; assuming valid",
            );
            true
        }
    }
}

fn narrate(ctx: &AppContext, msg: &str) {
    if ctx.global.silent {
        return;
    }
    let _ = ctx.ui.write_prompt(&format!("[codex-session] {msg}\n"));
}
