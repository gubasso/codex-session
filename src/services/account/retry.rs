#![allow(clippy::result_large_err)]

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;

use crate::clock::now_unix;
use crate::context::AppContext;
use crate::error::AppError;

use super::{
    AccountError, AccountId, cooldown,
    error::{AccountOutcomeLine, OutcomeState},
    failover, quota,
    registry::{AccountEntry, Registry},
    resolver::{self, ResolvedAccount},
    selector, token_refresh,
};

#[allow(clippy::too_many_lines)]
pub(crate) fn run_auto(ctx: &AppContext, argv: &[OsString]) -> Result<i32, AppError> {
    let max_retries = ctx.global.max_retries;
    let registry = Registry::from_config(&ctx.config);
    let accounts = registry.list()?;
    let usable_count = accounts
        .iter()
        .filter(|entry| selector::is_usable(ctx, &registry, entry))
        .count();
    if usable_count == 0 {
        return Err(AccountError::NoEligible {
            report: build_report(ctx, &registry, &accounts, Vec::new(), &HashSet::new()),
        }
        .into());
    }
    // Rotation cap. This is the auto path only (pinned accounts go through
    // `single_attempt` and never reach here), so the default `--max-retries 0`
    // deliberately means "try every eligible account once" rather than "one
    // attempt, no failover" — auto's whole purpose is to rotate across the pool
    // on a mid-run 401/429 (each account at most once). A non-zero `--max-retries`
    // caps total attempts at `(max_retries + 1)`, still bounded by the eligible
    // count. (The `--max-retries` help text is reworded in round 02.)
    let cap = if max_retries == 0 {
        usable_count
    } else {
        ((max_retries as usize) + 1).min(usable_count)
    };

    let signal_session = crate::commands::pass_through::SignalSession::install()?;
    let json_mode = crate::commands::pass_through::has_json_flag(argv);
    let mut tried = HashSet::new();
    let mut ran_report = Vec::new();
    let mut force_same: Option<ResolvedAccount> = None;
    let mut transient_attempts: HashMap<AccountId, u8> = HashMap::new();

    loop {
        if tried.len() >= cap && force_same.is_none() {
            break;
        }

        let resolved = if let Some(forced) = force_same.take() {
            forced
        } else {
            match resolver::resolve_for_exec(ctx, &tried) {
                Ok(resolved) => resolved,
                Err(AppError::Account(AccountError::NoEligible { .. })) => break,
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

        let events = super::codex_events::scan_events(&stdout_buf);
        let Some(classification) =
            failover::classify_run(&events, exit_code, json_mode, &stdout_buf, &stderr_buf)
        else {
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

        tracing::info!(
            op = "failover.match",
            account = %resolved.id,
            snippet = %classification.snippet,
            category = ?classification.category,
            reset_after_seconds = classification.reset_after_seconds,
            reset_source = classification.reset_source.map(failover::ResetSource::as_str),
        );

        let (kind_label, state, outcome) = match &classification.category {
            failover::Category::AuthFailure => {
                if first_use && try_refresh(ctx, &resolved.id) {
                    tracing::info!(
                        op = "token_refresh.ok",
                        account = %resolved.id,
                        "auth refresh succeeded; retrying same account"
                    );
                    force_same = Some(resolved.clone());
                    continue;
                }
                write_cooldown(
                    &registry,
                    &resolved.id,
                    "401",
                    &classification.snippet,
                    None,
                    None,
                )?;
                (
                    "401",
                    OutcomeState::AuthFailed401,
                    format!("401 auth failed: {}", classification.snippet),
                )
            }
            failover::Category::RateLimit(failover::RateLimitClass::Transient) => {
                let attempts = transient_attempts.entry(resolved.id.clone()).or_insert(0);
                *attempts += 1;
                if *attempts <= 3 {
                    let delay = classification.reset_after_seconds.unwrap_or(5).min(60);
                    tracing::info!(
                        op = "retry.backoff",
                        account = %resolved.id,
                        delay_secs = delay,
                        attempt = *attempts
                    );
                    ctx.ui.write_warning(&format!(
                        "warning: account '{}' rate limited (transient); backing off {delay}s…",
                        resolved.id
                    ))?;
                    std::thread::sleep(std::time::Duration::from_secs(delay));
                    force_same = Some(resolved.clone());
                    continue;
                }
                write_cooldown(
                    &registry,
                    &resolved.id,
                    "429",
                    &classification.snippet,
                    classification.reset_after_seconds,
                    classification.reset_source,
                )?;
                (
                    "429",
                    OutcomeState::RateLimited429,
                    format!("429 rate limit: {}", classification.snippet),
                )
            }
            failover::Category::RateLimit(failover::RateLimitClass::UsageLimitExhausted) => {
                write_cooldown(
                    &registry,
                    &resolved.id,
                    "429",
                    &classification.snippet,
                    classification.reset_after_seconds,
                    classification.reset_source,
                )?;
                (
                    "429",
                    OutcomeState::RateLimited429,
                    format!("429 rate limit: {}", classification.snippet),
                )
            }
            failover::Category::CreditExhausted => {
                // Rotation is correct here: credits are a workspace-level
                // overflow pool, but the block is per-account window
                // exhaustion — another account in the same workspace whose
                // window still has headroom keeps working (verified live, see
                // docs/upstream-codex.md §F9). The cooldown is reset-aware:
                // `credit_cooldown` derives it from the account's own usage
                // windows since the error itself carries no reset.
                let (reset, source) = credit_cooldown(ctx, &resolved.id);
                write_cooldown(
                    &registry,
                    &resolved.id,
                    "credits",
                    &classification.snippet,
                    reset,
                    source,
                )?;
                (
                    "credits",
                    OutcomeState::CreditExhausted,
                    format!("out of credits: {}", classification.snippet),
                )
            }
            failover::Category::ContextWindowExceeded
            | failover::Category::NoRolloutFound
            | failover::Category::ServerError
            | failover::Category::Unclassified => {
                return Err(codex_unhandled_error(ctx, &classification, exit_code));
            }
        };

        ran_report.push(account_outcome_line(
            ctx,
            &registry,
            &resolved.id,
            state,
            outcome,
        ));

        tracing::warn!(
            op = "account.switch",
            from = %resolved.id,
            reason = kind_label
        );
        if tried.len() < cap {
            ctx.ui.write_warning(&format!(
                "warning: account '{}' hit {kind_label}; rotating to next account…",
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

/// Auto-selection for an interactive TUI launch: pick the best eligible account
/// up front (pre-flight, the same selector dry-run uses) and run it with inherited
/// stdio. No output capture and no reactive 401/429 failover — codex owns the
/// terminal, so mid-session rotation is impossible and capture would break the
/// TUI's isatty check. Pre-flight selection (skips cooled-down/exhausted accounts,
/// scores by quota) is the defense.
pub(crate) fn run_auto_interactive(ctx: &AppContext, argv: &[OsString]) -> Result<i32, AppError> {
    let resolved = resolver::resolve_for_exec(ctx, &HashSet::new())?;
    tracing::info!(
        op = "retry.interactive",
        account = %resolved.id,
        "interactive passthrough: failover disabled, stdio inherited"
    );
    let signal_session = crate::commands::pass_through::SignalSession::install()?;
    let (exit_code, _stdout_buf, _stderr_buf) = crate::commands::pass_through::run_once(
        ctx,
        argv,
        &resolved,
        &signal_session,
        false,
        None,
    )?;
    // Best-effort recency bookkeeping, mirroring run_auto's success path. A write
    // failure must not turn a completed launch into an error.
    let registry = Registry::from_config(&ctx.config);
    if let Err(err) = registry.set_current(&resolved.id) {
        tracing::warn!(
            op = "last_account.write_failed",
            account = %resolved.id,
            error = %err,
        );
    }
    Ok(exit_code)
}

fn build_report(
    ctx: &AppContext,
    registry: &Registry,
    accounts: &[AccountEntry],
    ran_report: Vec<AccountOutcomeLine>,
    tried: &HashSet<AccountId>,
) -> Vec<AccountOutcomeLine> {
    let mut ran: HashMap<AccountId, AccountOutcomeLine> = ran_report
        .into_iter()
        .map(|line| (line.id.clone(), line))
        .collect();

    accounts
        .iter()
        .map(|entry| {
            if let Some(line) = ran.remove(&entry.id) {
                return line;
            }
            let (state, outcome) = fallback_state(ctx, registry, entry, tried);
            account_outcome_line(ctx, registry, &entry.id, state, outcome)
        })
        .collect()
}

pub(crate) fn write_cooldown(
    registry: &Registry,
    account: &AccountId,
    reason: &str,
    snippet: &str,
    reset_after_seconds: Option<u64>,
    reset_source: Option<failover::ResetSource>,
) -> Result<(), AppError> {
    let now_unix = now_unix();
    // Clamp to <= 6h against absurd server values, and to >= 1s so a genuine
    // cooldown (e.g. a server `retry_after: 0`) is never already-expired the
    // instant it is written, which would let a just-rotated account be reused
    // on the next invocation.
    let reset = reset_after_seconds.unwrap_or(300).clamp(1, 6 * 60 * 60);
    let reset_source = reset_source.map_or("fallback-300s", failover::ResetSource::as_str);
    let cd = cooldown::Cooldown {
        reset_at_unix: now_unix + reset,
        reason: format!("{reason} detected: {snippet:?}"),
        last_429_at_unix: now_unix,
        snippet_truncated: snippet.chars().take(256).collect(),
        reset_source: Some(reset_source.to_owned()),
    };
    let account_root = registry.account_dir(account);
    cooldown::write(&account_root, &cd).map_err(AccountError::from)?;
    tracing::info!(
        op = "cooldown.write",
        account = %account,
        reset_at_unix = cd.reset_at_unix,
        reason = %cd.reason,
        reset_after_secs = reset,
        reset_source,
    );
    Ok(())
}

/// Cooldown reset for a credit-exhausted account.
///
/// Codex reports "out of credits" when a plan window (5h/weekly) is fully
/// used AND the workspace has no purchased credits to overflow into; the
/// account recovers at the window reset *without* a top-up. Verified live
/// against `wham/usage` (`rate_limit_reached_type:
/// "workspace_owner_credits_depleted"` with `primary_window.used_percent:
/// 100`, while a same-workspace account with window headroom stayed
/// `allowed: true`), and corroborated by openai/codex#19830 whose error text
/// offers "purchase more credits OR try again at [reset time]". Full schema
/// facts live in docs/upstream-codex.md §F9.
///
/// So: cool down until the earliest reset among exhausted quota windows,
/// taken from the account's own usage data. A cached quota read is fine —
/// `reset_at_unix` is absolute, so staleness within the TTL doesn't skew the
/// duration. `None` (quota unavailable, API-key mode, or no exhausted window —
/// upstream has a known transient desync where credit state reads 0/absent)
/// falls back to `write_cooldown`'s 300s default: a short re-probe
/// self-corrects.
pub(crate) fn credit_cooldown(
    ctx: &AppContext,
    account: &AccountId,
) -> (Option<u64>, Option<failover::ResetSource>) {
    let ttl = std::time::Duration::from_secs(ctx.config.account.quota_ttl_secs);
    let Ok(quota::QuotaResult::Ok(quota)) = quota::get(ctx, account, ttl) else {
        return (None, None);
    };
    let reset_at = if let Some(five_hour) = quota
        .five_hour
        .as_ref()
        .filter(|window| window.percent_left <= 0.0)
    {
        five_hour.reset_at_unix
    } else if let Some(weekly) = quota
        .weekly
        .as_ref()
        .filter(|window| window.percent_left <= 0.0)
    {
        weekly.reset_at_unix
    } else {
        return (None, None);
    };
    let seconds = reset_at.saturating_sub(now_unix());
    if seconds == 0 {
        return (None, None);
    }
    (Some(seconds), Some(failover::ResetSource::ServerReset))
}

pub(crate) fn codex_unhandled_error(
    ctx: &AppContext,
    classification: &failover::Classification,
    exit_code: i32,
) -> AppError {
    let class = match classification.category {
        failover::Category::ContextWindowExceeded => "context-window-exceeded",
        failover::Category::NoRolloutFound => "no-rollout-found",
        failover::Category::ServerError => "server-error",
        failover::Category::Unclassified => "unclassified",
        failover::Category::RateLimit(_)
        | failover::Category::AuthFailure
        | failover::Category::CreditExhausted => {
            unreachable!("codex_unhandled_error only accepts unhandled categories")
        }
    };
    let log_glob = format!(
        "{}/codex-session.log*",
        crate::logging::log_dir_from_config(&ctx.config)
    );
    tracing::warn!(
        op = "codex.error.unhandled",
        class,
        snippet = %classification.snippet
    );
    AppError::CodexUnhandled {
        class,
        snippet: classification.snippet.clone(),
        log_glob,
        exit_code: u8::try_from(exit_code).unwrap_or(u8::MAX),
    }
}

pub(crate) fn account_outcome_line(
    ctx: &AppContext,
    registry: &Registry,
    account: &AccountId,
    state: OutcomeState,
    outcome: String,
) -> AccountOutcomeLine {
    let ttl = std::time::Duration::from_secs(ctx.config.account.quota_ttl_secs);
    let mut line = AccountOutcomeLine {
        id: account.clone(),
        outcome,
        state,
        five_hour_left: None,
        weekly_left: None,
        available_at_unix: None,
    };

    apply_quota(ctx, account, ttl, &mut line);
    apply_cooldown(registry, account, &mut line);
    line
}

/// Classify an alternate (non-owner) account for a `ResumeBlocked` report.
///
/// A genuinely usable account is reported as `NotAttempted` / "available for a
/// new thread"; an unusable one (no auth, expired token, active cooldown) gets
/// its real blocking state so the resume message never offers a fresh-exec path
/// that cannot actually run.
pub(crate) fn alternate_state(
    ctx: &AppContext,
    registry: &Registry,
    entry: &AccountEntry,
) -> (OutcomeState, String) {
    if let Some((state, outcome)) = unusable_outcome(ctx, registry, entry) {
        return (state, outcome);
    }
    (
        OutcomeState::NotAttempted,
        "available for a new thread".to_owned(),
    )
}

fn unusable_outcome(
    ctx: &AppContext,
    registry: &Registry,
    entry: &AccountEntry,
) -> Option<(OutcomeState, String)> {
    let reason = selector::unusable_reason(ctx, registry, entry)?;
    if reason == "no auth" {
        return Some((OutcomeState::NoAuth, "no auth".to_owned()));
    }
    if reason == "token expired" {
        return Some((OutcomeState::TokenExpired, "token expired".to_owned()));
    }
    if reason.starts_with("cooldown until ") {
        return Some((OutcomeState::Cooldown, reason));
    }
    Some((OutcomeState::AttemptedUnknown, reason))
}

fn fallback_state(
    ctx: &AppContext,
    registry: &Registry,
    entry: &AccountEntry,
    tried: &HashSet<AccountId>,
) -> (OutcomeState, String) {
    if let Some((state, outcome)) = unusable_outcome(ctx, registry, entry) {
        return (state, outcome);
    }
    if tried.contains(&entry.id) {
        (
            OutcomeState::AttemptedUnknown,
            "attempted but no terminal outcome recorded".to_owned(),
        )
    } else {
        (
            OutcomeState::NotAttempted,
            "not attempted (rotation cap reached)".to_owned(),
        )
    }
}

fn apply_quota(
    ctx: &AppContext,
    account: &AccountId,
    ttl: std::time::Duration,
    line: &mut AccountOutcomeLine,
) {
    let Ok(quota::QuotaResult::Ok(quota)) = quota::get(ctx, account, ttl) else {
        return;
    };
    line.five_hour_left = quota.five_hour.as_ref().map(|window| window.percent_left);
    line.weekly_left = quota.weekly.as_ref().map(|window| window.percent_left);

    let mut reset = None;
    let can_quota_set_state = !matches!(
        line.state,
        OutcomeState::RateLimited429
            | OutcomeState::AuthFailed401
            | OutcomeState::CreditExhausted
            | OutcomeState::Cooldown
            | OutcomeState::NoAuth
            | OutcomeState::TokenExpired
    );
    if let Some(five_hour) = quota.five_hour.as_ref()
        && five_hour.percent_left <= 0.0
    {
        if can_quota_set_state {
            line.state = OutcomeState::FiveHourExhausted;
            line.outcome = format!(
                "five-hour quota exhausted ({:.1}% left)",
                five_hour.percent_left
            );
        }
        reset = min_available(reset, five_hour.reset_at_unix);
    }
    if let Some(weekly) = quota.weekly.as_ref()
        && weekly.percent_left <= 0.0
    {
        if can_quota_set_state && !matches!(line.state, OutcomeState::FiveHourExhausted) {
            line.state = OutcomeState::WeeklyExhausted;
            line.outcome = format!("weekly quota exhausted ({:.1}% left)", weekly.percent_left);
        }
        reset = min_available(reset, weekly.reset_at_unix);
    }
    if !matches!(
        line.state,
        OutcomeState::RateLimited429
            | OutcomeState::AuthFailed401
            | OutcomeState::CreditExhausted
            | OutcomeState::Cooldown
            | OutcomeState::FiveHourExhausted
            | OutcomeState::WeeklyExhausted
            | OutcomeState::NoAuth
            | OutcomeState::TokenExpired
    ) {
        // A gate only fires for a window that is actually reported.
        let below_five = quota
            .five_hour
            .as_ref()
            .is_some_and(|window| window.percent_left <= ctx.config.account.five_hour_threshold);
        let below_weekly = quota
            .weekly
            .as_ref()
            .is_some_and(|window| window.percent_left <= ctx.config.account.weekly_floor);
        if below_five || below_weekly {
            let five_str = quota.five_hour.as_ref().map_or_else(
                || "n/a".to_owned(),
                |window| format!("{:.1}%", window.percent_left),
            );
            let weekly_str = quota.weekly.as_ref().map_or_else(
                || "n/a".to_owned(),
                |window| format!("{:.1}%", window.percent_left),
            );
            line.state = OutcomeState::BelowKnee;
            line.outcome = format!("below penalty knee (5h {five_str} / weekly {weekly_str} left)");
            // Below-knee is a soft scoring penalty, not a block: the account is
            // still selectable now. Do NOT record a window reset as
            // `available_at_unix` — that would render it as "back in <dur>" and
            // fold it into "earliest available", reintroducing hard-floor
            // semantics in the user guidance.
        }
    }
    line.available_at_unix = min_option(line.available_at_unix, reset);
}

fn apply_cooldown(registry: &Registry, account: &AccountId, line: &mut AccountOutcomeLine) {
    let Ok(Some(cd)) = cooldown::read(&registry.account_dir(account)) else {
        return;
    };
    if !cooldown::is_active(&cd, now_unix()) {
        return;
    }
    // `CreditExhausted` is protected here for the same reason as the 429/401
    // states: the credit arm itself writes a cooldown, and letting this read
    // relabel the line would downgrade the specific "out of credits" outcome
    // to a generic "cooldown active" one.
    if !matches!(
        line.state,
        OutcomeState::RateLimited429
            | OutcomeState::AuthFailed401
            | OutcomeState::CreditExhausted
            | OutcomeState::FiveHourExhausted
            | OutcomeState::WeeklyExhausted
    ) {
        line.state = OutcomeState::Cooldown;
        line.outcome = format!("cooldown active: {}", cd.reason);
    }
    line.available_at_unix = min_option(line.available_at_unix, Some(cd.reset_at_unix));
}

const fn min_option(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(if left < right { left } else { right }),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

const fn min_available(current: Option<u64>, candidate: u64) -> Option<u64> {
    if candidate == 0 {
        current
    } else {
        min_option(current, Some(candidate))
    }
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
