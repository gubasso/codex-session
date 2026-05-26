#![allow(clippy::result_large_err)]

use std::ffi::OsString;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::cli::account::AccountSelector;
use crate::context::AppContext;
use crate::error::AppError;

use super::{AccountError, cooldown, failover, quota, resolver, token_refresh};

#[allow(clippy::too_many_lines)]
pub(crate) fn run_with_retry(ctx: &AppContext, argv: &[OsString]) -> Result<i32, AppError> {
    let max_retries = ctx.global.max_retries;
    // Rotation requires the user to have opted into auto-selection. Any
    // single-account path (--account <name>, CODEX_SESSION_ACCOUNT=<name>,
    // state/last-account, config.account.pinned) would re-resolve the same
    // account on every retry, burning attempts and writing pointless
    // cooldown state. Short-circuit them all by inspecting the resolved
    // source on the first attempt.
    let pinned_name = match ctx.global.account.as_ref() {
        Some(AccountSelector::Named(name)) => Some(name.clone()),
        _ => None,
    };
    let env_pinned = std::env::var("CODEX_SESSION_ACCOUNT")
        .ok()
        .is_some_and(|raw| !raw.is_empty() && raw != "auto");
    let flag_auto = matches!(ctx.global.account.as_ref(), Some(AccountSelector::Auto));
    let env_auto = std::env::var("CODEX_SESSION_ACCOUNT")
        .ok()
        .is_some_and(|raw| raw == "auto");
    let rotation_opted_in = flag_auto || (env_auto && ctx.global.account.is_none());

    if max_retries > 0 && !rotation_opted_in {
        let warn_account = pinned_name.as_ref().map_or_else(
            || {
                if env_pinned {
                    "<env>".to_owned()
                } else {
                    "<resolved>".to_owned()
                }
            },
            ToString::to_string,
        );
        tracing::warn!(
            op = "account.pinned_retry_noop",
            account = %warn_account,
            max_retries,
            "ignoring --max-retries without --account auto: rotation requires --account auto"
        );
        ctx.ui.write_warning(
            "warning: --max-retries is ignored without --account auto; \
            retries against a single resolved account would just re-run it. \
            Use --account auto for failover rotation.",
        )?;
        return single_attempt(ctx, argv);
    }

    // Install signal forwarding ONCE for the wrapper invocation. Each retry
    // attempt reuses this guard via the shared `child_pid` slot. Installing
    // per attempt would leak iterator threads whose stale child_pid Arcs
    // (now `0`) would take the "no child" branch on a later signal and
    // terminate the wrapper instead of forwarding to the live child.
    let signal_session = crate::commands::pass_through::SignalSession::install()?;
    let registry = crate::services::account::registry::Registry::from_config(&ctx.config);
    let mut last_account = None;
    let mut retry_same: Option<resolver::ResolvedAccount> = None;
    for attempt in 0..=max_retries {
        let resolved = if let Some(forced) = retry_same.take() {
            forced
        } else {
            match resolver::resolve(ctx) {
                Ok(resolved) => resolved,
                Err(AppError::Account(AccountError::NoEligible))
                    if max_retries > 0 && attempt > 0 =>
                {
                    break;
                }
                Err(err) => return Err(err),
            }
        };
        last_account = Some(resolved.id.clone());
        tracing::info!(
            op = "retry.attempt",
            attempt,
            max_retries,
            account = %resolved.id,
            account_source = resolver::source_label(resolved.source),
        );
        let capture = max_retries > 0;
        let result = crate::commands::pass_through::run_once(
            ctx,
            argv,
            &resolved,
            &signal_session,
            capture,
        )?;
        let (exit_code, stdout_buf, stderr_buf) = result;

        let matched =
            failover::pick_priority(failover::scan(&stderr_buf), failover::scan(&stdout_buf));
        if let Some(matched) = matched {
            if max_retries == 0 {
                return Ok(exit_code);
            }

            if attempt == max_retries {
                return Ok(exit_code);
            }
            let pattern = failover::pattern_name(&matched);
            tracing::info!(
                op = "failover.match",
                account = %resolved.id,
                snippet = %matched.snippet,
                line_no = matched.line_no,
                pattern_index = matched.pattern_index,
                pattern,
                kind = ?matched.kind,
                attempt
            );
            let cooldown_reason = match matched.kind {
                failover::MatchKind::AuthFailure => {
                    let refresh_ok = match quota::resolve_auth_path(ctx, &resolved.id) {
                        Ok(auth_path) => match token_refresh::refresh_token(&auth_path) {
                            Ok(_) => true,
                            Err(e) => {
                                tracing::debug!(
                                    op = "token_refresh.fail",
                                    account = %resolved.id,
                                    err = %e
                                );
                                false
                            }
                        },
                        Err(e) => {
                            tracing::debug!(
                                op = "auth_path.fail",
                                account = %resolved.id,
                                err = %e
                            );
                            false
                        }
                    };

                    if refresh_ok {
                        tracing::info!(
                            op = "token_refresh.ok",
                            account = %resolved.id,
                            "auth refresh succeeded; retrying same account"
                        );
                        retry_same = Some(resolved.clone());
                        continue;
                    }
                    "401"
                }
                failover::MatchKind::RateLimit => "429",
            };

            let now_unix = now_unix();
            let cd = cooldown::Cooldown {
                reset_at_unix: now_unix + 300,
                reason: format!("{cooldown_reason} detected: {:?}", matched.snippet),
                last_429_at_unix: now_unix,
                snippet_truncated: matched.snippet.chars().take(256).collect(),
            };
            let account_root = registry.account_dir(&resolved.id);
            cooldown::write(&account_root, &cd).map_err(AccountError::from)?;
            tracing::info!(
                op = "cooldown.write",
                account = %resolved.id,
                reset_at_unix = cd.reset_at_unix,
                reason = %cd.reason
            );
            tracing::warn!(
                op = "account.switch",
                from = %resolved.id,
                attempt,
                reason = cooldown_reason
            );
            continue;
        }

        return Ok(exit_code);
    }

    tracing::error!(
        op = "retry.exhausted",
        max_retries,
        last_account = last_account
            .as_ref()
            .map_or_else(|| "unknown".to_owned(), ToString::to_string)
    );
    Err(AppError::Account(AccountError::NoEligible))
}

fn single_attempt(ctx: &AppContext, argv: &[OsString]) -> Result<i32, AppError> {
    let resolved = resolver::resolve(ctx)?;
    let signal_session = crate::commands::pass_through::SignalSession::install()?;
    let (exit_code, _stdout_buf, _stderr_buf) =
        crate::commands::pass_through::run_once(ctx, argv, &resolved, &signal_session, false)?;
    Ok(exit_code)
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}
