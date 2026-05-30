#![allow(clippy::result_large_err)]

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::context::AppContext;
use crate::error::AppError;

use super::{
    AccountError, AccountId, cooldown, failover, quota,
    registry::{AccountEntry, Registry},
    resolver::{self, ResolvedAccount},
    selector, token_refresh,
};

#[allow(clippy::too_many_lines)]
pub(crate) fn run_auto(ctx: &AppContext, argv: &[OsString]) -> Result<i32, AppError> {
    let max_retries = ctx.global.max_retries;
    let registry = Registry::from_config(&ctx.config);
    let accounts = registry.list()?;
    let eligible = accounts
        .iter()
        .filter(|entry| selector::is_usable(ctx, &registry, entry))
        .count();
    // Rotation cap. This is the auto path only (pinned accounts go through
    // `single_attempt` and never reach here), so the default `--max-retries 0`
    // deliberately means "try every eligible account once" rather than "one
    // attempt, no failover" — auto's whole purpose is to rotate across the pool
    // on a mid-run 401/429 (each account at most once). A non-zero `--max-retries`
    // caps total attempts at `(max_retries + 1)`, still bounded by the eligible
    // count. (The `--max-retries` help text is reworded in round 02.)
    let cap = if max_retries == 0 {
        eligible
    } else {
        ((max_retries as usize) + 1).min(eligible)
    };

    let signal_session = crate::commands::pass_through::SignalSession::install()?;
    let mut tried = HashSet::new();
    let mut ran_report = Vec::new();
    let mut force_same: Option<ResolvedAccount> = None;

    loop {
        if tried.len() >= cap && force_same.is_none() {
            break;
        }

        let resolved = if let Some(forced) = force_same.take() {
            forced
        } else {
            match resolver::resolve_for_exec(ctx, &tried) {
                Ok(resolved) => resolved,
                Err(AppError::Account(AccountError::NoEligible)) => break,
                Err(err) => return Err(err),
            }
        };
        let first_use = tried.insert(resolved.id.clone());
        tracing::info!(
            op = "retry.attempt",
            account = %resolved.id,
            account_source = resolver::source_label(resolved.source),
            tried = tried.len(),
            cap,
        );

        let (exit_code, stdout_buf, stderr_buf) = crate::commands::pass_through::run_once(
            ctx,
            argv,
            &resolved,
            &signal_session,
            true,
            None,
        )?;

        let matched =
            failover::pick_priority(failover::scan(&stderr_buf), failover::scan(&stdout_buf));
        let Some(matched) = matched else {
            // The child has already run and forwarded its stdout/stderr. Updating
            // `state/last-account` is best-effort bookkeeping (it only drives the
            // selector recency penalty) — a write failure here must not convert a
            // completed invocation into a wrapper error after output was emitted.
            if let Err(err) = registry.set_current(&resolved.id) {
                tracing::warn!(
                    op = "last_account.write_failed",
                    account = %resolved.id,
                    error = %err,
                );
            }
            return Ok(exit_code);
        };

        let pattern = failover::pattern_name(&matched);
        tracing::info!(
            op = "failover.match",
            account = %resolved.id,
            snippet = %matched.snippet,
            line_no = matched.line_no,
            pattern_index = matched.pattern_index,
            pattern,
            kind = ?matched.kind,
        );

        let (kind_label, outcome) = match matched.kind {
            failover::MatchKind::AuthFailure => {
                if first_use && try_refresh(ctx, &resolved.id) {
                    tracing::info!(
                        op = "token_refresh.ok",
                        account = %resolved.id,
                        "auth refresh succeeded; retrying same account"
                    );
                    force_same = Some(resolved.clone());
                    continue;
                }
                write_cooldown(&registry, &resolved.id, "401", &matched)?;
                ("401", format!("401 auth failed: {}", matched.snippet))
            }
            failover::MatchKind::RateLimit => {
                write_cooldown(&registry, &resolved.id, "429", &matched)?;
                ("429", format!("429 rate limit: {}", matched.snippet))
            }
        };

        ran_report.push(crate::services::account::error::AccountOutcomeLine {
            id: resolved.id.clone(),
            outcome,
        });

        tracing::warn!(
            op = "account.switch",
            from = %resolved.id,
            reason = kind_label
        );
        if tried.len() < cap {
            ctx.ui.write_warning(&format!(
                "account '{}' hit {kind_label}; rotating to next account…",
                resolved.id
            ))?;
        }
    }

    tracing::error!(
        op = "retry.exhausted",
        max_retries,
        tried = tried.len(),
        cap
    );
    Err(AccountError::AutoExhausted {
        report: build_report(ctx, &registry, &accounts, ran_report, &tried),
    }
    .into())
}

pub(crate) fn single_attempt(
    ctx: &AppContext,
    argv: &[OsString],
    resolved: &ResolvedAccount,
) -> Result<i32, AppError> {
    let signal_session = crate::commands::pass_through::SignalSession::install()?;
    let (exit_code, _stdout_buf, _stderr_buf) =
        crate::commands::pass_through::run_once(ctx, argv, resolved, &signal_session, false, None)?;
    Ok(exit_code)
}

fn build_report(
    ctx: &AppContext,
    registry: &Registry,
    accounts: &[AccountEntry],
    ran_report: Vec<crate::services::account::error::AccountOutcomeLine>,
    tried: &HashSet<AccountId>,
) -> Vec<crate::services::account::error::AccountOutcomeLine> {
    let mut ran: HashMap<AccountId, String> = ran_report
        .into_iter()
        .map(|line| (line.id, line.outcome))
        .collect();

    accounts
        .iter()
        .map(|entry| {
            let outcome = ran.remove(&entry.id).unwrap_or_else(|| {
                selector::skip_reason(ctx, registry, entry).unwrap_or_else(|| {
                    if tried.contains(&entry.id) {
                        "attempted but no terminal outcome recorded".to_owned()
                    } else {
                        "not attempted (rotation cap reached)".to_owned()
                    }
                })
            });
            crate::services::account::error::AccountOutcomeLine {
                id: entry.id.clone(),
                outcome,
            }
        })
        .collect()
}

fn write_cooldown(
    registry: &Registry,
    account: &AccountId,
    reason: &str,
    matched: &failover::Match,
) -> Result<(), AppError> {
    let now_unix = now_unix();
    let cd = cooldown::Cooldown {
        reset_at_unix: now_unix + 300,
        reason: format!("{reason} detected: {:?}", matched.snippet),
        last_429_at_unix: now_unix,
        snippet_truncated: matched.snippet.chars().take(256).collect(),
    };
    let account_root = registry.account_dir(account);
    cooldown::write(&account_root, &cd).map_err(AccountError::from)?;
    tracing::info!(
        op = "cooldown.write",
        account = %account,
        reset_at_unix = cd.reset_at_unix,
        reason = %cd.reason
    );
    Ok(())
}

fn try_refresh(ctx: &AppContext, account: &AccountId) -> bool {
    match quota::resolve_auth_path(ctx, account) {
        Ok(auth_path) => match token_refresh::refresh_token(&auth_path) {
            Ok(_) => true,
            Err(err) => {
                tracing::debug!(
                    op = "token_refresh.fail",
                    account = %account,
                    err = %err
                );
                false
            }
        },
        Err(err) => {
            tracing::debug!(
                op = "auth_path.fail",
                account = %account,
                err = %err
            );
            false
        }
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}
