//! `account quota` command.
#![allow(clippy::missing_errors_doc, clippy::result_large_err)]

use std::time::SystemTime;

use camino::Utf8PathBuf;

use crate::services::account::{AccountError, AccountId, quota, registry::Registry, selector};

pub(crate) fn run(
    ctx: &crate::context::AppContext,
    args: crate::cli::account::AccountQuotaArgs,
) -> Result<(), crate::error::AppError> {
    let registry = Registry::from_config(&ctx.config);

    if args.all {
        ctx.ui
            .write_warning("warning: --all is deprecated (all accounts are shown by default)")?;
    }

    let single_account = match ctx.global.account.as_ref() {
        Some(crate::cli::account::AccountSelector::Named(id)) => Some(id.clone()),
        _ => None,
    };

    let active = registry.current()?;
    let now = SystemTime::now();
    let mut entries = Vec::new();

    if let Some(ref target) = single_account {
        let is_active = active
            .as_ref()
            .is_some_and(|current| current.as_str() == target.as_str());
        let last_used_at = registry.expect_account_dir(target).ok().and_then(|_| {
            registry
                .list()
                .ok()
                .and_then(|list| list.into_iter().find(|entry| entry.id == *target))
                .and_then(|entry| entry.last_used_at)
        });
        entries.push(fetch_view(
            ctx,
            target,
            is_active,
            false,
            args.detail,
            active.as_ref(),
            last_used_at,
            now,
        )?);
    } else {
        for entry in registry.list()? {
            let is_active = active
                .as_ref()
                .is_some_and(|current| current.as_str() == entry.id.as_str());
            entries.push(fetch_view(
                ctx,
                &entry.id,
                is_active,
                true,
                args.detail,
                active.as_ref(),
                entry.last_used_at,
                now,
            )?);
        }
    }

    entries.sort_by(quota_sort_key);
    if single_account.is_none() {
        for (idx, entry) in entries.iter_mut().enumerate() {
            if entry.score.is_some() {
                entry.rank = Some(idx + 1);
            }
        }
    }

    if single_account.is_some() {
        ctx.ui
            .write_account_quota(&entries[0], args.format, args.detail)?;
    } else {
        ctx.ui
            .write_account_quota_many(&entries, args.format, args.detail)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn fetch_view(
    ctx: &crate::context::AppContext,
    account: &AccountId,
    active: bool,
    multi: bool,
    verbose: bool,
    lru: Option<&AccountId>,
    last_used_at: Option<SystemTime>,
    now: SystemTime,
) -> Result<crate::commands::account::AccountQuotaEntryView, AccountError> {
    let plan_bonus = quota::plan_bonus(ctx, account);
    let result = quota::refresh(ctx, account);

    match result {
        Ok(result) => {
            let meta = read_cache_meta(&cache_path(ctx, account));
            let scoring_raw = selector::score_from_quota_result(
                &result,
                &selector::ScoringParams {
                    plan_bonus,
                    last_used_at,
                    is_lru: lru.is_some_and(|id| id == account),
                    now,
                    five_hour_threshold: ctx.config.account.five_hour_threshold,
                    weekly_floor: ctx.config.account.weekly_floor,
                    five_hour_weight: ctx.config.account.five_hour_weight,
                },
            );
            Ok(view_from_result(
                account,
                active,
                result,
                &meta.unwrap_or_default(),
                Some(scoring_raw),
                verbose,
            ))
        }
        Err(err) => {
            if multi {
                Ok(crate::commands::account::AccountQuotaEntryView {
                    account: account.to_string(),
                    active,
                    mode: "error".to_owned(),
                    fetched_at_unix: 0,
                    ttl_secs: 0,
                    error: Some(AccountError::from(err).to_string()),
                    five_hour: None,
                    weekly: None,
                    score: None,
                    rank: None,
                    status_label: "quota_error".to_owned(),
                    scoring: None,
                })
            } else {
                Err(AccountError::from(err))
            }
        }
    }
}

fn view_from_result(
    account: &AccountId,
    active: bool,
    result: quota::QuotaResult,
    meta: &CacheMeta,
    scoring_raw: Option<selector::ScoreBreakdown>,
    verbose: bool,
) -> crate::commands::account::AccountQuotaEntryView {
    match result {
        quota::QuotaResult::Ok(quota) => {
            quota_view(account, active, &quota, meta, scoring_raw, verbose)
        }
        quota::QuotaResult::ApiKeyMode => crate::commands::account::AccountQuotaEntryView {
            account: account.to_string(),
            active,
            mode: "api-key".to_owned(),
            fetched_at_unix: meta.fetched_at_unix,
            ttl_secs: meta.ttl_secs.max(300),
            error: None,
            five_hour: None,
            weekly: None,
            score: scoring_raw.as_ref().map(|sc| sc.total),
            rank: None,
            status_label: "api-key".to_owned(),
            scoring: scoring_raw.map(|value| scoring_view(&value, verbose)),
        },
    }
}

fn scoring_view(
    value: &selector::ScoreBreakdown,
    verbose: bool,
) -> crate::commands::account::AccountScoringView {
    let _ = verbose;
    crate::commands::account::AccountScoringView {
        base: value.base,
        plan_bonus: value.plan_bonus,
        recency: value.recency,
        recency_label: value.recency_label.clone(),
        avail_score: value.avail_score,
        five_hour_pct: value.five_hour_pct,
        weekly_pct: value.weekly_pct,
        five_hour_weight: value.five_hour_weight,
        weekly_pressure: value.weekly_pressure,
        fh_pressure: value.fh_pressure,
        pressure_label: value.pressure_label.clone(),
        total: value.total,
        eligible: value.eligible,
        ineligible_reason: value.ineligible_reason.clone(),
        tie_five_hour: value.tie_five_hour,
    }
}

fn quota_view(
    account: &AccountId,
    active: bool,
    quota: &quota::Quota,
    meta: &CacheMeta,
    scoring_raw: Option<selector::ScoreBreakdown>,
    verbose: bool,
) -> crate::commands::account::AccountQuotaEntryView {
    crate::commands::account::AccountQuotaEntryView {
        account: account.to_string(),
        active,
        mode: "oauth".to_owned(),
        fetched_at_unix: meta.fetched_at_unix,
        ttl_secs: meta.ttl_secs,
        error: None,
        five_hour: Some(crate::commands::account::AccountQuotaWindowView {
            percent_left: quota.five_hour.percent_left,
            reset_at_unix: quota.five_hour.reset_at_unix,
        }),
        weekly: Some(crate::commands::account::AccountQuotaWindowView {
            percent_left: quota.weekly.percent_left,
            reset_at_unix: quota.weekly.reset_at_unix,
        }),
        score: scoring_raw.as_ref().map(|sc| sc.total),
        rank: None,
        status_label: "ok".to_owned(),
        scoring: scoring_raw.map(|value| scoring_view(&value, verbose)),
    }
}

#[derive(Debug, Default)]
struct CacheMeta {
    fetched_at_unix: u64,
    ttl_secs: u64,
}

fn cache_path(ctx: &crate::context::AppContext, account: &AccountId) -> Utf8PathBuf {
    ctx.config
        .paths
        .state_dir
        .join("cache")
        .join("quota")
        .join(format!("{}.json", account.as_str()))
}

fn read_cache_meta(path: &Utf8PathBuf) -> Option<CacheMeta> {
    let bytes = std::fs::read(path.as_std_path()).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    Some(CacheMeta {
        fetched_at_unix: value.get("fetched_at_unix")?.as_u64()?,
        ttl_secs: value.get("ttl_secs")?.as_u64()?,
    })
}

fn quota_sort_key(
    left: &crate::commands::account::AccountQuotaEntryView,
    right: &crate::commands::account::AccountQuotaEntryView,
) -> std::cmp::Ordering {
    match (left.score, right.score) {
        (Some(left_score), Some(right_score)) => right_score
            .partial_cmp(&left_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.account.cmp(&right.account)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => left.account.cmp(&right.account),
    }
}
