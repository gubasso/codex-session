//! `account quota` command.
#![allow(clippy::missing_errors_doc, clippy::result_large_err)]

use std::sync::Arc;
use std::time::SystemTime;

use camino::Utf8PathBuf;
use tokio::task::JoinSet;

use crate::commands::account::{
    AccountQuotaAggregateView, AccountQuotaAggregateWindowView, AccountQuotaEntryView,
    AccountQuotaWindowView,
};
use crate::context::AppContext;
use crate::services::account::{AccountError, AccountId, quota, registry::Registry, selector};
use crate::ui::spinner::{SpinnerGroup, should_show_spinner};

#[allow(clippy::too_many_lines)]
pub(crate) async fn run(
    ctx: Arc<AppContext>,
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
    let spinners = SpinnerGroup::new(should_show_spinner(ctx.as_ref(), args.format, false));

    if let Some(ref target) = single_account {
        let spinner = spinners.add(&format!("Fetching quota for \"{target}\"..."));
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
        let result = fetch_view(
            ctx.as_ref(),
            target,
            is_active,
            false,
            args.detail,
            active.as_ref(),
            last_used_at,
            now,
        );
        match result.await {
            Ok(view) => {
                spinner.finish_ok(target.as_str());
                entries.push(view);
            }
            Err(err) => {
                spinner.finish_err(&format!("{target} — {err}"));
                return Err(err.into());
            }
        }
    } else {
        let mut set = JoinSet::new();
        for entry in registry.list()? {
            let is_active = active
                .as_ref()
                .is_some_and(|current| current.as_str() == entry.id.as_str());
            let account = entry.id;
            let last_used_at = entry.last_used_at;
            let spinner = spinners.add(&format!("Fetching quota for \"{account}\"..."));
            set.spawn({
                let ctx = Arc::clone(&ctx);
                let active = active.clone();
                async move {
                    let result = fetch_view(
                        ctx.as_ref(),
                        &account,
                        is_active,
                        true,
                        args.detail,
                        active.as_ref(),
                        last_used_at,
                        now,
                    )
                    .await;
                    match &result {
                        Ok(_) => spinner.finish_ok(account.as_str()),
                        Err(err) => spinner.finish_err(&format!("{account} — {err}")),
                    }
                    result
                }
            });
        }

        while let Some(result) = set.join_next().await {
            entries.push(
                result
                    .map_err(|err| {
                        crate::error::AppError::Other(anyhow::anyhow!(
                            "quota task join failed: {err}"
                        ))
                    })?
                    .map_err(crate::error::AppError::from)?,
            );
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
    let aggregate = if single_account.is_none() {
        aggregate_windows(&entries)
    } else {
        None
    };

    if single_account.is_some() {
        ctx.ui
            .write_account_quota(&entries[0], args.format, args.detail)?;
    } else {
        ctx.ui
            .write_account_quota_many(&entries, aggregate.as_ref(), args.format, args.detail)?;
    }
    Ok(())
}

fn aggregate_windows(entries: &[AccountQuotaEntryView]) -> Option<AccountQuotaAggregateView> {
    fn mean<F>(rows: &[&AccountQuotaEntryView], pick: F) -> Option<AccountQuotaAggregateWindowView>
    where
        F: Fn(&AccountQuotaEntryView) -> Option<f64>,
    {
        let vals: Vec<f64> = rows.iter().filter_map(|r| pick(r)).collect();
        if vals.is_empty() {
            return None;
        }
        let sum: f64 = vals.iter().sum();
        #[allow(clippy::cast_precision_loss)]
        Some(AccountQuotaAggregateWindowView {
            percent_left: sum / vals.len() as f64,
        })
    }

    let oauth: Vec<&AccountQuotaEntryView> = entries.iter().filter(|e| e.mode == "oauth").collect();
    if oauth.len() < 2 {
        return None;
    }

    let five_hour = mean(&oauth, |e| e.five_hour.as_ref().map(|w| w.percent_left));
    let weekly = mean(&oauth, |e| e.weekly.as_ref().map(|w| w.percent_left));

    if five_hour.is_none() && weekly.is_none() {
        return None;
    }

    Some(AccountQuotaAggregateView {
        accounts_counted: oauth.len(),
        five_hour,
        weekly,
    })
}

#[allow(clippy::too_many_arguments)]
async fn fetch_view(
    ctx: &crate::context::AppContext,
    account: &AccountId,
    active: bool,
    multi: bool,
    verbose: bool,
    lru: Option<&AccountId>,
    last_used_at: Option<SystemTime>,
    now: SystemTime,
) -> Result<AccountQuotaEntryView, AccountError> {
    let plan_bonus = quota::plan_bonus(ctx, account);
    let result = quota::refresh(ctx, account).await;

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
                Ok(AccountQuotaEntryView {
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
) -> AccountQuotaEntryView {
    match result {
        quota::QuotaResult::Ok(quota) => {
            quota_view(account, active, &quota, meta, scoring_raw, verbose)
        }
        quota::QuotaResult::ApiKeyMode => AccountQuotaEntryView {
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
) -> AccountQuotaEntryView {
    AccountQuotaEntryView {
        account: account.to_string(),
        active,
        mode: "oauth".to_owned(),
        fetched_at_unix: meta.fetched_at_unix,
        ttl_secs: meta.ttl_secs,
        error: None,
        five_hour: Some(AccountQuotaWindowView {
            percent_left: quota.five_hour.percent_left,
            reset_at_unix: quota.five_hour.reset_at_unix,
        }),
        weekly: Some(AccountQuotaWindowView {
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

#[cfg(test)]
mod tests {
    use super::aggregate_windows;
    use crate::commands::account::{AccountQuotaEntryView, AccountQuotaWindowView};

    fn entry(mode: &str, fh: Option<f64>, wk: Option<f64>) -> AccountQuotaEntryView {
        AccountQuotaEntryView {
            account: mode.to_owned(),
            active: false,
            mode: mode.to_owned(),
            fetched_at_unix: 0,
            ttl_secs: 0,
            error: None,
            five_hour: fh.map(|percent_left| AccountQuotaWindowView {
                percent_left,
                reset_at_unix: 0,
            }),
            weekly: wk.map(|percent_left| AccountQuotaWindowView {
                percent_left,
                reset_at_unix: 0,
            }),
            score: None,
            rank: None,
            status_label: String::new(),
            scoring: None,
        }
    }

    #[test]
    fn aggregates_two_oauth_entries() {
        let aggregate = aggregate_windows(&[
            entry("oauth", Some(60.0), Some(40.0)),
            entry("oauth", Some(80.0), Some(70.0)),
        ]);

        assert_eq!(aggregate.as_ref().map(|agg| agg.accounts_counted), Some(2));
        assert!(
            aggregate
                .as_ref()
                .and_then(|agg| agg.five_hour.as_ref())
                .is_some_and(|window| (window.percent_left - 70.0).abs() < 1e-9)
        );
        assert!(
            aggregate
                .as_ref()
                .and_then(|agg| agg.weekly.as_ref())
                .is_some_and(|window| (window.percent_left - 55.0).abs() < 1e-9)
        );
    }

    #[test]
    fn ignores_single_oauth_entry() {
        assert!(aggregate_windows(&[entry("oauth", Some(60.0), Some(40.0))]).is_none());
    }

    #[test]
    fn counts_only_oauth_entries() {
        let aggregate = aggregate_windows(&[
            entry("oauth", Some(60.0), Some(40.0)),
            entry("oauth", Some(80.0), Some(70.0)),
            entry("api-key", None, None),
            entry("error", None, None),
        ]);

        assert_eq!(aggregate.as_ref().map(|agg| agg.accounts_counted), Some(2));
        assert!(
            aggregate
                .as_ref()
                .and_then(|agg| agg.five_hour.as_ref())
                .is_some_and(|window| (window.percent_left - 70.0).abs() < 1e-9)
        );
        assert!(
            aggregate
                .as_ref()
                .and_then(|agg| agg.weekly.as_ref())
                .is_some_and(|window| (window.percent_left - 55.0).abs() < 1e-9)
        );
    }

    #[test]
    fn aggregates_windows_independently() {
        let aggregate = aggregate_windows(&[
            entry("oauth", Some(60.0), Some(40.0)),
            entry("oauth", Some(80.0), None),
            entry("oauth", Some(100.0), Some(70.0)),
        ]);

        assert_eq!(aggregate.as_ref().map(|agg| agg.accounts_counted), Some(3));
        assert!(
            aggregate
                .as_ref()
                .and_then(|agg| agg.five_hour.as_ref())
                .is_some_and(|window| (window.percent_left - 80.0).abs() < 1e-9)
        );
        assert!(
            aggregate
                .as_ref()
                .and_then(|agg| agg.weekly.as_ref())
                .is_some_and(|window| (window.percent_left - 55.0).abs() < 1e-9)
        );
    }

    #[test]
    fn returns_none_when_all_oauth_windows_are_missing() {
        assert!(
            aggregate_windows(&[entry("oauth", None, None), entry("oauth", None, None),]).is_none()
        );
    }
}
