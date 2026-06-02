#![allow(clippy::result_large_err)]

use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::format_description::well_known::Rfc3339;

use super::{AccountError, AccountId, registry::Registry};
use crate::clock::now_unix;

const API_KEY_TTL_SECS: u64 = 300;
const DEFAULT_WHAM_USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";

pub(crate) enum QuotaResult {
    Ok(Quota),
    ApiKeyMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct Quota {
    pub(crate) five_hour: Window,
    pub(crate) weekly: Window,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct Window {
    pub(crate) percent_left: f64,
    pub(crate) reset_at_unix: u64,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum QuotaError {
    #[error("{0}")]
    Network(String),
    #[error("http status {0}")]
    HttpStatus(u16),
    #[error("missing rate_limit")]
    ParseMissingRateLimit,
    #[error("missing window: {0}")]
    ParseMissingWindow(&'static str),
    #[error("{0}")]
    AuthMissing(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheEntry {
    fetched_at_unix: u64,
    ttl_secs: u64,
    body: CacheBody,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CacheBody {
    Ok { five_hour: Window, weekly: Window },
    ApiKey,
}

#[derive(Debug)]
enum CacheLoad {
    Missing,
    Entry(CacheEntry),
    Malformed,
}

#[derive(Debug)]
enum AuthKind {
    OAuth {
        access_token: String,
        account_id: String,
        plan_bonus: i64,
    },
    ApiKey,
}

pub(crate) fn get(
    ctx: &crate::context::AppContext,
    account: &AccountId,
    ttl: Duration,
) -> Result<QuotaResult, QuotaError> {
    let cache = load_cache(&cache_path(ctx, account));
    match &cache {
        CacheLoad::Entry(entry) if cache_is_fresh(entry) => {
            tracing::info!(op = "quota.cache_hit", account = %account);
            return Ok(cache_body_to_result(entry.body.clone()));
        }
        CacheLoad::Malformed => {
            tracing::info!(op = "quota.cache_miss", account = %account, reason = "malformed");
        }
        CacheLoad::Missing | CacheLoad::Entry(_) => {
            tracing::info!(op = "quota.cache_miss", account = %account);
        }
    }

    let _ = ttl;
    block_on_refresh(ctx, account)
}

// Sync bridge for `quota::get`, which is on the synchronous exec hot path
// (selector -> quota::get). On that path this nests inside the `block_in_place`
// that `dispatch` wraps `pass_through::run` in; nested `block_in_place` is valid
// only under the multi-thread runtime. See `crate::runtime::block_on`.
fn block_on_refresh(
    ctx: &crate::context::AppContext,
    account: &AccountId,
) -> Result<QuotaResult, QuotaError> {
    crate::runtime::block_on(refresh(ctx, account))
}

pub(crate) async fn refresh(
    ctx: &crate::context::AppContext,
    account: &AccountId,
) -> Result<QuotaResult, QuotaError> {
    fetch(ctx, account).await
}

async fn fetch(
    ctx: &crate::context::AppContext,
    account: &AccountId,
) -> Result<QuotaResult, QuotaError> {
    match fetch_inner(ctx, account).await {
        Err(QuotaError::HttpStatus(401)) => {
            tracing::info!(
                op = "quota.fetch",
                account = %account,
                outcome = "401_retry",
                "attempting token refresh before retrying"
            );
            let auth_path = resolve_auth_path(ctx, account)?;
            let auth_path_owned = auth_path.clone();
            let refreshed = tokio::task::spawn_blocking(move || {
                super::token_refresh::refresh_token(&auth_path_owned)
            })
            .await;
            match refreshed {
                Ok(Ok(_)) => {
                    tracing::info!(
                        op = "quota.token_refresh",
                        account = %account,
                        outcome = "ok"
                    );
                    fetch_inner(ctx, account).await
                }
                Ok(Err(err)) => {
                    tracing::warn!(
                        op = "quota.token_refresh",
                        account = %account,
                        error = %err,
                        "token refresh failed; surfacing original 401"
                    );
                    Err(QuotaError::HttpStatus(401))
                }
                Err(join_err) => {
                    tracing::warn!(
                        op = "quota.token_refresh",
                        account = %account,
                        error = %join_err,
                        "token refresh task panicked; surfacing original 401"
                    );
                    Err(QuotaError::HttpStatus(401))
                }
            }
        }
        other => other,
    }
}

async fn fetch_inner(
    ctx: &crate::context::AppContext,
    account: &AccountId,
) -> Result<QuotaResult, QuotaError> {
    let auth = resolve_auth(ctx, account)?;
    match auth {
        AuthKind::ApiKey => {
            let entry = CacheEntry {
                fetched_at_unix: now_unix(),
                ttl_secs: API_KEY_TTL_SECS,
                body: CacheBody::ApiKey,
            };
            write_cache(&cache_path(ctx, account), &entry)?;
            tracing::info!(op = "quota.fetch", account = %account, outcome = "api_key");
            Ok(QuotaResult::ApiKeyMode)
        }
        AuthKind::OAuth {
            access_token,
            account_id,
            ..
        } => {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .map_err(|err| {
                    tracing::info!(op = "quota.fetch", account = %account, outcome = "network");
                    QuotaError::Network(err.to_string())
                })?;
            let url = base_url();

            for attempt in 0..=1 {
                let response = client
                    .get(url)
                    .header(
                        reqwest::header::AUTHORIZATION,
                        format!("Bearer {access_token}"),
                    )
                    .header("ChatGPT-Account-Id", &account_id)
                    .header(reqwest::header::ACCEPT, "application/json")
                    .header(reqwest::header::ORIGIN, "https://chatgpt.com")
                    .header(reqwest::header::REFERER, "https://chatgpt.com/")
                    .header(reqwest::header::USER_AGENT, "Mozilla/5.0")
                    .send()
                    .await
                    .map_err(|err| {
                        tracing::info!(op = "quota.fetch", account = %account, outcome = "network");
                        QuotaError::Network(err.to_string())
                    })?;

                let status = response.status();
                if status.is_server_error() {
                    if attempt == 0 {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                    tracing::info!(
                        op = "quota.fetch",
                        account = %account,
                        outcome = "http_status",
                        status = status.as_u16()
                    );
                    return Err(QuotaError::HttpStatus(status.as_u16()));
                }
                if !status.is_success() {
                    tracing::info!(
                        op = "quota.fetch",
                        account = %account,
                        outcome = "http_status",
                        status = status.as_u16()
                    );
                    return Err(QuotaError::HttpStatus(status.as_u16()));
                }

                let body = response.bytes().await.map_err(|err| {
                    tracing::info!(op = "quota.fetch", account = %account, outcome = "network");
                    QuotaError::Network(err.to_string())
                })?;
                let quota = parse_quota_body(&body).inspect_err(|_| {
                    tracing::info!(op = "quota.fetch", account = %account, outcome = "parse");
                })?;
                let entry = CacheEntry {
                    fetched_at_unix: now_unix(),
                    ttl_secs: ctx.config.account.quota_ttl_secs,
                    body: CacheBody::Ok {
                        five_hour: quota.five_hour.clone(),
                        weekly: quota.weekly.clone(),
                    },
                };
                write_cache(&cache_path(ctx, account), &entry)?;
                tracing::info!(op = "quota.fetch", account = %account, outcome = "ok");
                return Ok(QuotaResult::Ok(quota));
            }

            unreachable!("retry loop must return on success or error");
        }
    }
}

pub(crate) fn plan_bonus(ctx: &crate::context::AppContext, account: &AccountId) -> i64 {
    match resolve_auth(ctx, account) {
        Ok(AuthKind::OAuth { plan_bonus, .. }) => plan_bonus,
        Ok(AuthKind::ApiKey) | Err(_) => 0,
    }
}

impl From<QuotaError> for AccountError {
    fn from(err: QuotaError) -> Self {
        match err {
            QuotaError::Network(detail) => Self::QuotaFetch {
                detail: format!("network: {detail}"),
            },
            QuotaError::HttpStatus(code) => Self::QuotaFetch {
                detail: format!("http {code}"),
            },
            QuotaError::AuthMissing(detail) => Self::QuotaFetch {
                detail: format!("auth: {detail}"),
            },
            QuotaError::Io(io) => Self::QuotaFetch {
                detail: format!("io: {io}"),
            },
            QuotaError::ParseMissingRateLimit => Self::QuotaParse {
                detail: "missing rate_limit".to_owned(),
            },
            QuotaError::ParseMissingWindow(name) => Self::QuotaParse {
                detail: format!("missing window: {name}"),
            },
        }
    }
}

fn base_url() -> &'static str {
    static URL: OnceLock<String> = OnceLock::new();
    URL.get_or_init(|| {
        std::env::var("CODEX_SESSION_WHAM_USAGE_URL")
            .ok()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| DEFAULT_WHAM_USAGE_URL.to_owned())
    })
    .as_str()
}

fn cache_path(ctx: &crate::context::AppContext, account: &AccountId) -> Utf8PathBuf {
    ctx.config
        .paths
        .state_dir
        .join("cache")
        .join("quota")
        .join(format!("{}.json", account.as_str()))
}

fn load_cache(path: &Utf8PathBuf) -> CacheLoad {
    match std::fs::read(path.as_std_path()) {
        Ok(bytes) => serde_json::from_slice::<CacheEntry>(&bytes)
            .map_or(CacheLoad::Malformed, CacheLoad::Entry),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => CacheLoad::Missing,
        Err(_) => CacheLoad::Missing,
    }
}

fn write_cache(path: &Utf8PathBuf, entry: &CacheEntry) -> Result<(), QuotaError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent.as_std_path())?;
    }
    let bytes = serde_json::to_vec_pretty(entry)
        .map_err(|err| QuotaError::Io(std::io::Error::other(err.to_string())))?;
    crate::adapters::fs::atomic_write(path, &bytes).map_err(|err| match err {
        crate::adapters::fs::FsError::Io { source, .. } => QuotaError::Io(source),
        other => QuotaError::AuthMissing(other.to_string()),
    })
}

fn cache_is_fresh(entry: &CacheEntry) -> bool {
    now_unix() < entry.fetched_at_unix.saturating_add(entry.ttl_secs)
}

const fn cache_body_to_result(body: CacheBody) -> QuotaResult {
    match body {
        CacheBody::Ok { five_hour, weekly } => QuotaResult::Ok(Quota { five_hour, weekly }),
        CacheBody::ApiKey => QuotaResult::ApiKeyMode,
    }
}

fn parse_quota_body(body: &[u8]) -> Result<Quota, QuotaError> {
    let value: Value =
        serde_json::from_slice(body).map_err(|_| QuotaError::ParseMissingRateLimit)?;
    let root = value
        .get("rate_limit")
        .or_else(|| value.get("rate_limits"))
        .and_then(Value::as_object)
        .ok_or(QuotaError::ParseMissingRateLimit)?;

    Ok(Quota {
        five_hour: parse_window(
            &[
                root.get("five_hour"),
                root.get("primary_window"),
                root.get("primary"),
            ],
            "five_hour",
        )?,
        weekly: parse_window(
            &[
                root.get("weekly"),
                root.get("secondary_window"),
                root.get("secondary"),
            ],
            "weekly",
        )?,
    })
}

fn parse_window(candidates: &[Option<&Value>], name: &'static str) -> Result<Window, QuotaError> {
    let window = candidates
        .iter()
        .find_map(|c| c.filter(|v| v.is_object()))
        .ok_or(QuotaError::ParseMissingWindow(name))?;

    let percent_left = window
        .get("percent_left")
        .and_then(Value::as_f64)
        .or_else(|| {
            window
                .get("used_percent")
                .or_else(|| window.get("usedPercent"))
                .and_then(Value::as_f64)
                .map(|used| 100.0 - used)
        })
        .ok_or(QuotaError::ParseMissingWindow(name))?;
    let reset_at_unix = parse_reset_at_unix(window).unwrap_or(0);

    Ok(Window {
        percent_left,
        reset_at_unix,
    })
}

fn parse_reset_at_unix(window: &Value) -> Option<u64> {
    if let Some(value) = window.get("reset_time_ms") {
        if let Some(ms) = value.as_u64() {
            return Some(ms / 1000);
        }
        if let Some(ms) = value.as_f64()
            && ms.is_finite()
            && ms >= 0.0
        {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "ms is finite and non-negative; flooring to u64 is the intended truncation"
            )]
            let secs = (ms / 1000.0).floor() as u64;
            return Some(secs);
        }
    }

    if let Some(value) = window.get("resetsAt") {
        if let Some(ts) = value.as_u64() {
            return Some(ts);
        }
        if let Some(ts) = value.as_f64()
            && ts.is_finite()
            && ts >= 0.0
        {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "ts is finite and non-negative; flooring to u64 is the intended truncation"
            )]
            return Some(ts.floor() as u64);
        }
    }

    if let Some(value) = window.get("reset_at") {
        if let Some(ts) = value.as_u64() {
            return Some(ts);
        }
        if let Some(ts) = value.as_f64()
            && ts.is_finite()
            && ts >= 0.0
        {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "ts is finite and non-negative; flooring to u64 is the intended truncation"
            )]
            return Some(ts.floor() as u64);
        }
        if let Some(raw) = value.as_str()
            && let Ok(parsed) = time::OffsetDateTime::parse(raw, &Rfc3339)
        {
            let ts = parsed.unix_timestamp();
            #[allow(
                clippy::cast_sign_loss,
                reason = "guarded by ts >= 0 check before the cast"
            )]
            return (ts >= 0).then_some(ts as u64);
        }
    }

    None
}

pub(crate) fn resolve_auth_path(
    ctx: &crate::context::AppContext,
    account: &AccountId,
) -> Result<Utf8PathBuf, QuotaError> {
    let registry = Registry::from_config(&ctx.config);
    let current_group = crate::services::session::group_id::current(ctx)
        .ok()
        .map(|resolved| {
            registry
                .account_dir(account)
                .join("groups")
                .join(resolved.id.as_str())
                .join("auth.json")
        })
        .filter(|path| is_regular_file(path));
    let newest_group = newest_group_auth_path(&registry, account)?;
    let seed = registry.group_auth_seed_path(account);

    current_group
        .or(newest_group)
        .or_else(|| is_regular_file(&seed).then_some(seed.clone()))
        .ok_or_else(|| QuotaError::AuthMissing(format!("no auth.json under {}", account.as_str())))
}

fn resolve_auth(
    ctx: &crate::context::AppContext,
    account: &AccountId,
) -> Result<AuthKind, QuotaError> {
    let auth_path = resolve_auth_path(ctx, account)?;

    let bytes = crate::services::auth::secure_file_read(&auth_path).map_err(|err| match err {
        crate::services::auth::AuthError::Io { source, .. } => QuotaError::Io(source),
        other => QuotaError::AuthMissing(other.to_string()),
    })?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|err| QuotaError::AuthMissing(format!("malformed auth.json: {err}")))?;

    let access_token = value
        .get("tokens")
        .and_then(|tokens| tokens.get("access_token"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());
    let account_id = value
        .get("tokens")
        .and_then(|tokens| tokens.get("account_id"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());

    match (access_token, account_id) {
        (Some(access_token), Some(account_id)) => Ok(AuthKind::OAuth {
            access_token: access_token.to_owned(),
            account_id: account_id.to_owned(),
            plan_bonus: parse_plan_bonus(&value),
        }),
        _ => Ok(AuthKind::ApiKey),
    }
}

fn newest_group_auth_path(
    registry: &Registry,
    account: &AccountId,
) -> Result<Option<Utf8PathBuf>, QuotaError> {
    let groups = registry.account_dir(account).join("groups");
    let entries = match std::fs::read_dir(groups.as_std_path()) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(QuotaError::Io(err)),
    };

    let mut newest: Option<(SystemTime, Utf8PathBuf)> = None;
    for entry in entries {
        let entry = entry?;
        let Ok(path) = Utf8PathBuf::try_from(entry.path()) else {
            continue;
        };
        let Ok(metadata) = std::fs::symlink_metadata(path.as_std_path()) else {
            continue;
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            continue;
        }
        let auth = path.join("auth.json");
        if !is_regular_file(&auth) {
            continue;
        }
        let Ok(modified) = std::fs::metadata(auth.as_std_path()).and_then(|meta| meta.modified())
        else {
            continue;
        };
        if newest.as_ref().is_none_or(|(best, _)| &modified > best) {
            newest = Some((modified, auth));
        }
    }

    Ok(newest.map(|(_, path)| path))
}

fn is_regular_file(path: &camino::Utf8Path) -> bool {
    std::fs::symlink_metadata(path.as_std_path())
        .is_ok_and(|metadata| !metadata.file_type().is_symlink() && metadata.is_file())
}

fn parse_plan_bonus(value: &Value) -> i64 {
    let plan = plan_type_from_jwt(value).or_else(|| {
        value
            .get("tokens")
            .and_then(|tokens| tokens.get("plan"))
            .and_then(Value::as_str)
            .map(str::to_ascii_lowercase)
    });
    match plan.as_deref() {
        Some("enterprise") => 30,
        Some("pro" | "team" | "plus") => 20,
        _ => 0,
    }
}

fn plan_type_from_jwt(value: &Value) -> Option<String> {
    use base64::Engine as _;

    let jwt = value
        .get("tokens")
        .and_then(|tokens| tokens.get("access_token"))
        .and_then(Value::as_str)?;
    let payload = jwt.split('.').nth(1)?;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let claims: Value = serde_json::from_slice(&decoded).ok()?;
    claims
        .get("https://api.openai.com/auth")
        .and_then(|auth| auth.get("chatgpt_plan_type"))
        .and_then(Value::as_str)
        .map(str::to_ascii_lowercase)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn parse_new_primary_secondary_shape() {
        let body = br#"{
            "rate_limits": {
                "primary": {
                    "usedPercent": 26.6,
                    "resetsAt": 1716393600,
                    "windowDurationMins": 300
                },
                "secondary": {
                    "usedPercent": 12.9,
                    "resetsAt": 1716998400,
                    "windowDurationMins": 10080
                }
            }
        }"#;
        let quota = parse_quota_body(body).unwrap();
        assert!((quota.five_hour.percent_left - 73.4).abs() < 0.01);
        assert!((quota.weekly.percent_left - 87.1).abs() < 0.01);
        assert_eq!(quota.five_hour.reset_at_unix, 1_716_393_600);
        assert_eq!(quota.weekly.reset_at_unix, 1_716_998_400);
    }

    #[test]
    fn parse_used_percent_conversion() {
        let body = br#"{
            "rate_limit": {
                "primary": { "usedPercent": 0.0, "resetsAt": 100 },
                "secondary": { "usedPercent": 100.0, "resetsAt": 200 }
            }
        }"#;
        let quota = parse_quota_body(body).unwrap();
        assert!((quota.five_hour.percent_left - 100.0).abs() < f64::EPSILON);
        assert!((quota.weekly.percent_left - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_percent_left_preferred_over_used_percent() {
        let body = br#"{
            "rate_limit": {
                "five_hour": { "percent_left": 80.0, "usedPercent": 50.0, "reset_time_ms": 1000 },
                "weekly": { "percent_left": 90.0, "usedPercent": 50.0, "reset_time_ms": 2000 }
            }
        }"#;
        let quota = parse_quota_body(body).unwrap();
        assert!((quota.five_hour.percent_left - 80.0).abs() < f64::EPSILON);
        assert!((quota.weekly.percent_left - 90.0).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_resets_at_seconds() {
        let body = br#"{
            "rate_limit": {
                "five_hour": { "percent_left": 50.0, "resetsAt": 1716393600 },
                "weekly": { "percent_left": 60.0, "resetsAt": 1716998400 }
            }
        }"#;
        let quota = parse_quota_body(body).unwrap();
        assert_eq!(quota.five_hour.reset_at_unix, 1_716_393_600);
        assert_eq!(quota.weekly.reset_at_unix, 1_716_998_400);
    }

    #[test]
    fn parse_resets_at_float() {
        let body = br#"{
            "rate_limit": {
                "five_hour": { "percent_left": 50.0, "resetsAt": 1716393600.7 },
                "weekly": { "percent_left": 60.0, "resetsAt": 1716998400.3 }
            }
        }"#;
        let quota = parse_quota_body(body).unwrap();
        assert_eq!(quota.five_hour.reset_at_unix, 1_716_393_600);
        assert_eq!(quota.weekly.reset_at_unix, 1_716_998_400);
    }

    #[test]
    fn parse_reset_time_ms_preferred_over_resets_at() {
        let body = br#"{
            "rate_limit": {
                "five_hour": {
                    "percent_left": 50.0,
                    "reset_time_ms": 1716393600000,
                    "resetsAt": 9999999999
                },
                "weekly": {
                    "percent_left": 60.0,
                    "reset_time_ms": 1716998400000,
                    "resetsAt": 9999999999
                }
            }
        }"#;
        let quota = parse_quota_body(body).unwrap();
        assert_eq!(quota.five_hour.reset_at_unix, 1_716_393_600);
        assert_eq!(quota.weekly.reset_at_unix, 1_716_998_400);
    }

    #[test]
    fn parse_real_api_shape_with_used_percent_snake_case() {
        let body = br#"{
            "rate_limit": {
                "primary_window": {
                    "used_percent": 1,
                    "limit_window_seconds": 18000,
                    "reset_after_seconds": 18000,
                    "reset_at": 1779813200
                },
                "secondary_window": {
                    "used_percent": 0,
                    "limit_window_seconds": 604800,
                    "reset_after_seconds": 604800,
                    "reset_at": 1780400000
                }
            }
        }"#;
        let quota = parse_quota_body(body).unwrap();
        assert!((quota.five_hour.percent_left - 99.0).abs() < f64::EPSILON);
        assert!((quota.weekly.percent_left - 100.0).abs() < f64::EPSILON);
        assert_eq!(quota.five_hour.reset_at_unix, 1_779_813_200);
        assert_eq!(quota.weekly.reset_at_unix, 1_780_400_000);
    }

    #[test]
    fn parse_reset_at_integer_preferred_over_string() {
        let body = br#"{
            "rate_limit": {
                "five_hour": { "percent_left": 50.0, "reset_at": 1716393600 },
                "weekly": { "percent_left": 60.0, "reset_at": 1716998400 }
            }
        }"#;
        let quota = parse_quota_body(body).unwrap();
        assert_eq!(quota.five_hour.reset_at_unix, 1_716_393_600);
        assert_eq!(quota.weekly.reset_at_unix, 1_716_998_400);
    }
}
