//! `account quota` command.
#![allow(clippy::missing_errors_doc, clippy::result_large_err)]

use camino::Utf8PathBuf;

use crate::services::account::{AccountError, AccountId, quota, registry::Registry};

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
    if let Some(target) = single_account {
        let is_active = active
            .as_ref()
            .is_some_and(|current| current.as_str() == target.as_str());
        let view = fetch_view(ctx, args, &target, is_active, false)?;
        ctx.ui.write_account_quota(&view, args.format)?;
    } else {
        let mut entries = Vec::new();
        for entry in registry.list()? {
            let is_active = active
                .as_ref()
                .is_some_and(|current| current.as_str() == entry.id.as_str());
            entries.push(fetch_view(ctx, args, &entry.id, is_active, true)?);
        }
        entries.sort_by(quota_sort_key);
        ctx.ui.write_account_quota_many(&entries, args.format)?;
    }
    Ok(())
}

fn fetch_view(
    ctx: &crate::context::AppContext,
    args: crate::cli::account::AccountQuotaArgs,
    account: &AccountId,
    active: bool,
    multi: bool,
) -> Result<crate::commands::account::AccountQuotaEntryView, AccountError> {
    let result = if args.live {
        quota::refresh(ctx, account)
    } else {
        quota::get(
            ctx,
            account,
            std::time::Duration::from_secs(ctx.config.account.quota_ttl_secs),
        )
    };

    match result {
        Ok(result) => {
            let meta = read_cache_meta(&cache_path(ctx, account));
            Ok(view_from_result(
                account,
                active,
                result,
                &meta.unwrap_or_default(),
                args.live,
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
                    stale: false,
                    live: args.live,
                    error: Some(AccountError::from(err).to_string()),
                    five_hour: None,
                    weekly: None,
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
    live: bool,
) -> crate::commands::account::AccountQuotaEntryView {
    match result {
        quota::QuotaResult::Ok(quota) => quota_view(account, active, &quota, meta, live, false),
        quota::QuotaResult::Stale(quota) => quota_view(account, active, &quota, meta, live, true),
        quota::QuotaResult::ApiKeyMode => crate::commands::account::AccountQuotaEntryView {
            account: account.to_string(),
            active,
            mode: "api-key".to_owned(),
            fetched_at_unix: meta.fetched_at_unix,
            ttl_secs: meta.ttl_secs.max(300),
            stale: false,
            live,
            error: None,
            five_hour: None,
            weekly: None,
        },
    }
}

fn quota_view(
    account: &AccountId,
    active: bool,
    quota: &quota::Quota,
    meta: &CacheMeta,
    live: bool,
    stale: bool,
) -> crate::commands::account::AccountQuotaEntryView {
    crate::commands::account::AccountQuotaEntryView {
        account: account.to_string(),
        active,
        mode: "oauth".to_owned(),
        fetched_at_unix: meta.fetched_at_unix,
        ttl_secs: meta.ttl_secs,
        stale,
        live,
        error: None,
        five_hour: Some(crate::commands::account::AccountQuotaWindowView {
            percent_left: quota.five_hour.percent_left,
            reset_at_unix: quota.five_hour.reset_at_unix,
        }),
        weekly: Some(crate::commands::account::AccountQuotaWindowView {
            percent_left: quota.weekly.percent_left,
            reset_at_unix: quota.weekly.reset_at_unix,
        }),
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
    match (
        left.five_hour.as_ref().map(|window| window.percent_left),
        right.five_hour.as_ref().map(|window| window.percent_left),
    ) {
        (Some(left_pct), Some(right_pct)) => right_pct
            .partial_cmp(&left_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.account.cmp(&right.account)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => left.account.cmp(&right.account),
    }
}
