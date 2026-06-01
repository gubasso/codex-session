//! `account health` command.
#![allow(clippy::missing_errors_doc, clippy::result_large_err)]

use std::sync::Arc;
use std::time::SystemTime;

use serde_json::Value;
use tokio::task::JoinSet;

use crate::cli::account::{AccountHealthArgs, AccountSelector};
use crate::commands::account::{AccountHealthEntryView, AccountHealthView, AccountScoringView};
use crate::context::AppContext;
use crate::services::account::{
    AccountError, AccountId, cooldown, gate, quota,
    registry::{AccountEntry, Registry},
    selector,
    token_expiry::{TokenExpiry, token_expiry_from_auth},
};
use crate::ui::spinner::{SpinnerGroup, should_show_spinner};

pub(crate) async fn run(
    ctx: Arc<AppContext>,
    args: AccountHealthArgs,
) -> Result<(), crate::error::AppError> {
    if matches!(ctx.global.account, Some(AccountSelector::Auto)) {
        return Err(crate::error::AppError::Usage(clap::Error::raw(
            clap::error::ErrorKind::InvalidValue,
            "--account auto is not supported for account health; pass a concrete account name",
        )));
    }

    if !args.fast {
        gate::validate_ping_config_recipe(ctx.as_ref())?;
        ctx.ensure_child_version()?;
    }

    let registry = Registry::from_config(&ctx.config);
    let active = registry.current()?;
    let mut entries = Vec::new();
    let now = SystemTime::now();

    let accounts: Vec<AccountEntry> =
        if let Some(AccountSelector::Named(target)) = &ctx.global.account {
            vec![
                registry
                    .list()?
                    .into_iter()
                    .find(|entry| &entry.id == target)
                    .ok_or_else(|| AccountError::NotFound {
                        name: target.clone(),
                        path: registry.account_dir(target),
                    })?,
            ]
        } else {
            registry.list()?
        };

    let spinners = SpinnerGroup::new(should_show_spinner(ctx.as_ref(), args.format, args.fast));
    let mut set = JoinSet::new();
    for entry in accounts {
        let spinner = spinners.add(&format!("Checking account \"{}\"...", entry.id));
        set.spawn({
            let ctx = Arc::clone(&ctx);
            let active = active.clone();
            async move {
                let view = build_entry(BuildEntryInput {
                    ctx,
                    entry,
                    active,
                    fast: args.fast,
                    now,
                })
                .await;
                if view.status == "live" && view.token == "ok" {
                    spinner.finish_ok(&format!("Account \"{}\" healthy", view.account));
                } else if view.status == "live" {
                    spinner.finish_err(&format!(
                        "Account \"{}\" token {}",
                        view.account, view.token
                    ));
                } else {
                    spinner.finish_err(&format!("Account \"{}\" {}", view.account, view.status));
                }
                view
            }
        });
    }

    while let Some(result) = set.join_next().await {
        entries.push(result.map_err(|err| {
            crate::error::AppError::Other(anyhow::anyhow!("health task join failed: {err}"))
        })?);
    }

    entries.sort_by(|left, right| match (left.score, right.score) {
        (Some(l), Some(r)) => r
            .partial_cmp(&l)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.account.cmp(&right.account)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => left.account.cmp(&right.account),
    });
    for (idx, entry) in entries.iter_mut().enumerate() {
        if entry.score.is_some() {
            entry.rank = Some(idx + 1);
        }
    }

    ctx.ui
        .write_account_health(&AccountHealthView { entries }, args.format, args.detail)?;
    Ok(())
}

struct BuildEntryInput {
    ctx: Arc<AppContext>,
    entry: AccountEntry,
    active: Option<AccountId>,
    fast: bool,
    now: SystemTime,
}

async fn build_entry(input: BuildEntryInput) -> AccountHealthEntryView {
    let BuildEntryInput {
        ctx,
        entry,
        active,
        fast,
        now,
    } = input;
    let registry = Registry::from_config(&ctx.config);
    let account = &entry.id;
    let auth_path = registry.group_auth_seed_path(account);
    let token_state = read_token_state(&auth_path);
    let plan_bonus = quota::plan_bonus(ctx.as_ref(), account);
    let plan = match plan_bonus {
        30 => "Enterprise",
        20 => "Pro or Team",
        _ => "Free or Unknown",
    }
    .to_owned();
    let cooldown = cooldown::read(&registry.account_dir(account))
        .unwrap_or(None)
        .is_some_and(|state| cooldown::is_active(&state, now_unix()));

    // `OpenAI` uses single-use refresh tokens: a refresh invalidates the old one
    // (see token_refresh.rs). `quota::refresh` may perform a 401 refresh that
    // rotates the active auth file mid-flight; the probe (run concurrently below)
    // reads the same credentials and would race quota on that single-use token,
    // falsely reporting `invalid` on a healthy account. To let only quota own the
    // rotation, snapshot the active auth file's access token before the join; if
    // quota rotated it, re-probe against the freshly-rotated file afterwards.
    let active_auth = if fast {
        None
    } else {
        quota::resolve_auth_path(ctx.as_ref(), account).ok()
    };
    let pre_access_token = active_auth.as_deref().and_then(read_access_token);

    let (quota_tuple, probe) = tokio::join!(
        fetch_quota(ctx.as_ref(), account, fast),
        fetch_probe(ctx.as_ref(), account, fast),
    );
    let (quota_result, fetched_at_unix, status) = quota_tuple;

    let probe = match active_auth.as_deref() {
        // Only "live" means quota actually refreshed; compare the access token to
        // confirm a rotation happened (a non-401 fetch leaves the file untouched).
        Some(path) if status == "live" => {
            let post_access_token = read_access_token(path);
            if post_access_token.is_some() && post_access_token != pre_access_token {
                fetch_probe_with_auth(ctx.as_ref(), path, fast).await
            } else {
                probe
            }
        }
        _ => probe,
    };

    let scoring_raw = quota_result.as_ref().map(|result| {
        selector::score_from_quota_result(
            result,
            &selector::ScoringParams {
                plan_bonus,
                last_used_at: entry.last_used_at,
                is_lru: active.as_ref().is_some_and(|value| value == account),
                now,
                five_hour_threshold: ctx.config.account.five_hour_threshold,
                weekly_floor: ctx.config.account.weekly_floor,
                five_hour_weight: ctx.config.account.five_hour_weight,
            },
        )
    });

    let (score, score_label, scoring) = scoring_raw.as_ref().map_or_else(
        || (None, "—".to_owned(), None),
        |raw| {
            (
                Some(raw.total),
                format!("{:.2}", raw.total),
                Some(scoring_view_from_breakdown(raw)),
            )
        },
    );

    AccountHealthEntryView {
        account: account.to_string(),
        token: match probe.0 {
            Some(true) => "ok".to_owned(),
            Some(false) => "invalid".to_owned(),
            None => "unknown".to_owned(),
        },
        token_detail: match token_state {
            TokenExpiry::ExpiresAt(ts) => format!("exp={ts}, probe={}", probe.1),
            TokenExpiry::Missing => format!("missing_access_token, probe={}", probe.1),
            TokenExpiry::Malformed => format!("malformed_access_token, probe={}", probe.1),
        },
        plan,
        score,
        rank: None,
        status,
        active: active.as_ref().is_some_and(|value| value == account),
        cooldown,
        last_used: entry
            .last_used_at
            .and_then(|ts| ts.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs()),
        fetched_at_unix,
        score_label,
        scoring,
    }
}

async fn fetch_quota(
    ctx: &crate::context::AppContext,
    account: &AccountId,
    fast: bool,
) -> (Option<quota::QuotaResult>, u64, String) {
    if fast {
        match read_quota_from_cache(ctx, account) {
            Some((result, fetched)) => (Some(result), fetched, "cache only".to_owned()),
            None => (None, 0, "cache missing".to_owned()),
        }
    } else {
        let pre_fetched = read_cache_fetched_at(ctx, account).unwrap_or(0);
        quota::refresh(ctx, account).await.map_or_else(
            |_| (None, pre_fetched, "fetch failed".to_owned()),
            |result| {
                let post_fetched = read_cache_fetched_at(ctx, account).unwrap_or(pre_fetched);
                (Some(result), post_fetched, "live".to_owned())
            },
        )
    }
}

async fn fetch_probe(
    ctx: &crate::context::AppContext,
    account: &AccountId,
    fast: bool,
) -> (Option<bool>, String) {
    if fast {
        return (None, "skipped".to_owned());
    }
    match gate::probe_token(ctx, account).await {
        Ok((value, detail)) => (
            value,
            if detail.is_empty() {
                "ok".to_owned()
            } else {
                detail
            },
        ),
        Err(err) => (None, err.to_string()),
    }
}

/// Re-probe against a specific auth file (the one `quota::refresh` just rotated)
/// instead of the account seed, so the probe reflects the post-rotation token.
async fn fetch_probe_with_auth(
    ctx: &crate::context::AppContext,
    auth_source: &camino::Utf8Path,
    fast: bool,
) -> (Option<bool>, String) {
    if fast {
        return (None, "skipped".to_owned());
    }
    match gate::probe_token_with_auth(ctx, auth_source).await {
        Ok((value, detail)) => (
            value,
            if detail.is_empty() {
                "ok".to_owned()
            } else {
                detail
            },
        ),
        Err(err) => (None, err.to_string()),
    }
}

/// Read the OAuth `access_token` from an auth file, if present and non-empty.
/// Used to detect whether `quota::refresh` rotated the token during the
/// concurrent quota∥probe window.
fn read_access_token(path: &camino::Utf8Path) -> Option<String> {
    let bytes = std::fs::read(path.as_std_path()).ok()?;
    let auth: Value = serde_json::from_slice(&bytes).ok()?;
    auth.get("tokens")
        .and_then(|tokens| tokens.get("access_token"))
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
        .map(ToOwned::to_owned)
}

fn read_token_state(path: &camino::Utf8Path) -> TokenExpiry {
    let bytes = std::fs::read(path.as_std_path()).ok();
    let Some(bytes) = bytes else {
        return TokenExpiry::Missing;
    };
    let auth: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    token_expiry_from_auth(&auth)
}

fn read_quota_from_cache(
    ctx: &crate::context::AppContext,
    account: &AccountId,
) -> Option<(quota::QuotaResult, u64)> {
    let path = cache_path(ctx, account);
    let bytes = std::fs::read(path.as_std_path()).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    let fetched = value
        .get("fetched_at_unix")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let body = value.get("body")?;
    let kind = body.get("kind").and_then(Value::as_str)?;
    match kind {
        "api_key" => Some((quota::QuotaResult::ApiKeyMode, fetched)),
        "ok" => {
            let fh = body.get("five_hour")?;
            let wk = body.get("weekly")?;
            Some((
                quota::QuotaResult::Ok(quota::Quota {
                    five_hour: quota::Window {
                        percent_left: fh.get("percent_left")?.as_f64()?,
                        reset_at_unix: fh.get("reset_at_unix")?.as_u64()?,
                    },
                    weekly: quota::Window {
                        percent_left: wk.get("percent_left")?.as_f64()?,
                        reset_at_unix: wk.get("reset_at_unix")?.as_u64()?,
                    },
                }),
                fetched,
            ))
        }
        _ => None,
    }
}

fn read_cache_fetched_at(ctx: &crate::context::AppContext, account: &AccountId) -> Option<u64> {
    let bytes = std::fs::read(cache_path(ctx, account).as_std_path()).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    value.get("fetched_at_unix").and_then(Value::as_u64)
}

fn cache_path(ctx: &crate::context::AppContext, account: &AccountId) -> camino::Utf8PathBuf {
    ctx.config
        .paths
        .state_dir
        .join("cache")
        .join("quota")
        .join(format!("{}.json", account.as_str()))
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

fn scoring_view_from_breakdown(raw: &selector::ScoreBreakdown) -> AccountScoringView {
    AccountScoringView {
        base: raw.base,
        plan_bonus: raw.plan_bonus,
        recency: raw.recency,
        recency_label: raw.recency_label.clone(),
        avail_score: raw.avail_score,
        five_hour_pct: raw.five_hour_pct,
        weekly_pct: raw.weekly_pct,
        five_hour_weight: raw.five_hour_weight,
        weekly_pressure: raw.weekly_pressure,
        fh_pressure: raw.fh_pressure,
        pressure_label: raw.pressure_label.clone(),
        total: raw.total,
        eligible: raw.eligible,
        ineligible_reason: raw.ineligible_reason.clone(),
        tie_five_hour: raw.tie_five_hour,
    }
}
