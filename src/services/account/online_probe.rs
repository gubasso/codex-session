//! Mitigated concurrent token-probe + quota refresh for a single account.
//!
//! `OpenAI` uses single-use refresh tokens: a refresh invalidates the old one
//! (see `token_refresh.rs`). Running a token probe concurrently with a quota
//! refresh on the same account can race on that single-use token — both
//! `gate::probe_token` (via `persist_probe_rotation`) and `quota::refresh`
//! (via its 401 rotation) may write the seed, and last-writer-wins can retain a
//! refresh token whose peer already consumed it server-side (re-orphaning).
//!
//! This module centralizes the mitigation used by both `account health` and
//! `doctor --online`:
//!   1. Up-front, refresh an expired-at-entry seed exactly once, so neither
//!      quota nor the probe rotates within the join window (closes the common
//!      race).
//!   2. Snapshot the active auth's access token before the join; if quota's live
//!      refresh rotated it, re-probe against the freshly-rotated file afterwards.
//!
//! `account health` (`commands/account/health.rs`) reuses the leaf helpers here
//! but keeps its own richer orchestration (fast/cache modes, status strings,
//! token-state view). `doctor` uses [`probe_and_quota`] directly for the simple
//! always-live diagnostic case.
#![allow(clippy::result_large_err)]

use serde_json::Value;

use crate::clock::now_unix;
use crate::context::AppContext;
use crate::error::AppError;
use crate::services::account::id::AccountId;
use crate::services::account::quota::{QuotaError, QuotaResult};
use crate::services::account::registry::Registry;
use crate::services::account::token_expiry::{TokenExpiry, token_expiry_from_auth};
use crate::services::account::{gate, quota, token_refresh};

/// Near-expiry skew: refresh if the token expires within this window.
pub(crate) const TOKEN_REFRESH_SKEW_SECS: u64 = 60;

/// Run the token probe and quota refresh concurrently for one account, applying
/// the single-use refresh-token race mitigation described in the module docs.
///
/// Returns the probe result (already reconciled against any quota rotation) and
/// the quota refresh result. The caller maps these into its own view/check
/// vocabulary.
pub(crate) async fn probe_and_quota(
    ctx: &AppContext,
    account: &AccountId,
) -> (
    Result<(Option<bool>, String), AppError>,
    Result<QuotaResult, QuotaError>,
) {
    let registry = Registry::from_config(&ctx.config);
    let seed = registry.group_auth_seed_path(account);
    let active_auth = quota::resolve_auth_path(ctx, account).ok();

    // Step 1: refresh an expired seed once up-front so neither side rotates
    // inside the join.
    refresh_seed_if_needed(account, &seed, active_auth.as_deref()).await;

    // Step 2: snapshot the pre-join token, then run probe ∥ quota.
    let pre_access_token = active_auth.as_deref().and_then(read_access_token);
    let probe_auth = quota_resolved_probe_auth(&seed, active_auth.as_deref());
    let (quota_result, probe) = tokio::join!(
        quota::refresh(ctx, account),
        run_probe(ctx, account, probe_auth),
    );

    // Step 2 (cont.): if quota performed a live refresh that rotated the token,
    // re-probe against the rotated file so the verdict reflects the new token.
    let probe = reconcile(
        ctx,
        account,
        &seed,
        active_auth.as_deref(),
        quota_result.is_ok(),
        pre_access_token.as_deref(),
        probe,
    )
    .await;

    (probe, quota_result)
}

async fn run_probe(
    ctx: &AppContext,
    account: &AccountId,
    auth_source: Option<&camino::Utf8Path>,
) -> Result<(Option<bool>, String), AppError> {
    match auth_source {
        Some(path) => gate::probe_token_with_auth(ctx, path).await,
        None => gate::probe_token(ctx, account).await,
    }
}

/// Re-probe against the quota-rotated auth file when quota performed a live
/// refresh that changed the access token; otherwise return the original probe.
async fn reconcile(
    ctx: &AppContext,
    account: &AccountId,
    seed: &camino::Utf8Path,
    active_auth: Option<&camino::Utf8Path>,
    quota_live: bool,
    pre_access_token: Option<&str>,
    probe: Result<(Option<bool>, String), AppError>,
) -> Result<(Option<bool>, String), AppError> {
    // A failed probe is conclusive on its own; do not paper over it.
    if probe.is_err() || !quota_live {
        return probe;
    }
    let Some(path) = active_auth else {
        return probe;
    };
    let post_access_token = read_access_token(path);
    if !token_rotated(pre_access_token, post_access_token.as_deref()) {
        return probe;
    }
    if path != seed
        && let Err(err) = copy_auth_file(path, seed)
    {
        tracing::warn!(
            op = "online_probe.token_refresh",
            account = %account,
            path = %path,
            error = %err,
            "failed to sync quota-rotated auth back to seed"
        );
    }
    gate::probe_token_with_auth(ctx, path).await
}

/// Refresh the seed once if its token is expired/near-expiry at entry, so the
/// concurrent probe and quota refresh do not both rotate the single-use token.
async fn refresh_seed_if_needed(
    account: &AccountId,
    seed: &camino::Utf8Path,
    active_auth: Option<&camino::Utf8Path>,
) {
    if !token_needs_refresh(&read_token_state(seed), now_unix()) {
        return;
    }
    let seed_owned = seed.to_path_buf();
    let refreshed =
        tokio::task::spawn_blocking(move || token_refresh::refresh_token(&seed_owned)).await;
    match refreshed {
        Ok(Ok(_)) => {
            tracing::info!(op = "online_probe.token_refresh", account = %account, outcome = "ok");
            if let Some(path) = active_auth
                && path != seed
                && let Err(err) = copy_auth_file(seed, path)
            {
                tracing::warn!(
                    op = "online_probe.token_refresh",
                    account = %account,
                    path = %path,
                    error = %err,
                    "failed to copy refreshed seed to quota auth path"
                );
            }
        }
        Ok(Err(err)) => {
            tracing::warn!(
                op = "online_probe.token_refresh",
                account = %account,
                outcome = "failed",
                error = %err,
                "up-front token refresh failed; online checks will report account state"
            );
        }
        Err(err) => {
            tracing::warn!(
                op = "online_probe.token_refresh",
                account = %account,
                outcome = "join_failed",
                error = %err,
                "up-front token refresh task failed; online checks will report account state"
            );
        }
    }
}

/// Choose which auth file the probe should read: the account seed by default,
/// or a *distinct* group `auth.json` resolved by quota, so the verdict reflects
/// the token quota will actually use.
pub(crate) fn quota_resolved_probe_auth<'a>(
    seed: &'a camino::Utf8Path,
    active_auth: Option<&'a camino::Utf8Path>,
) -> Option<&'a camino::Utf8Path> {
    active_auth.filter(|path| *path != seed)
}

/// Read the OAuth `access_token` from an auth file, if present and non-empty.
/// Used to detect whether a refresh rotated the token during the concurrent
/// quota ∥ probe window.
pub(crate) fn read_access_token(path: &camino::Utf8Path) -> Option<String> {
    let bytes = std::fs::read(path.as_std_path()).ok()?;
    let auth: Value = serde_json::from_slice(&bytes).ok()?;
    auth.get("tokens")
        .and_then(|tokens| tokens.get("access_token"))
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
        .map(ToOwned::to_owned)
}

pub(crate) fn read_token_state(path: &camino::Utf8Path) -> TokenExpiry {
    let Some(bytes) = std::fs::read(path.as_std_path()).ok() else {
        return TokenExpiry::Missing;
    };
    let auth: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    token_expiry_from_auth(&auth)
}

pub(crate) const fn token_needs_refresh(state: &TokenExpiry, now_unix: u64) -> bool {
    match state {
        TokenExpiry::ExpiresAt(ts) => *ts <= now_unix.saturating_add(TOKEN_REFRESH_SKEW_SECS),
        TokenExpiry::Missing | TokenExpiry::Malformed => false,
    }
}

pub(crate) fn token_rotated(pre: Option<&str>, post: Option<&str>) -> bool {
    post.is_some() && post != pre
}

pub(crate) fn copy_auth_file(
    source: &camino::Utf8Path,
    target: &camino::Utf8Path,
) -> Result<(), AppError> {
    let bytes = crate::services::auth::secure_file_read(source)?;
    crate::adapters::fs::atomic_write(target, &bytes)
        .map_err(crate::services::auth::AuthError::from)?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use camino::Utf8PathBuf;

    use super::*;

    fn write_auth_file(contents: &str) -> Utf8PathBuf {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.keep().join("auth.json");
        std::fs::write(&path, contents).unwrap();
        Utf8PathBuf::from_path_buf(path).unwrap()
    }

    #[test]
    fn token_rotated_detects_new_or_changed_post_token() {
        assert!(token_rotated(None, Some("new")));
        assert!(token_rotated(Some("old"), Some("new")));
        assert!(!token_rotated(Some("same"), Some("same")));
        assert!(!token_rotated(Some("old"), None));
        assert!(!token_rotated(None, None));
    }

    #[test]
    fn token_needs_refresh_for_expired_or_near_expiry_oauth_token() {
        let now = 1_000;
        assert!(token_needs_refresh(&TokenExpiry::ExpiresAt(now - 1), now));
        assert!(token_needs_refresh(
            &TokenExpiry::ExpiresAt(now + TOKEN_REFRESH_SKEW_SECS),
            now,
        ));
        assert!(!token_needs_refresh(
            &TokenExpiry::ExpiresAt(now + TOKEN_REFRESH_SKEW_SECS + 1),
            now,
        ));
    }

    #[test]
    fn token_needs_refresh_ignores_missing_or_malformed_tokens() {
        assert!(!token_needs_refresh(&TokenExpiry::Missing, 1_000));
        assert!(!token_needs_refresh(&TokenExpiry::Malformed, 1_000));
    }

    #[test]
    fn read_access_token_returns_non_empty_oauth_access_token() {
        let path = write_auth_file(
            r#"{"tokens":{"access_token":"access-token","refresh_token":"refresh-token"}}"#,
        );
        assert_eq!(read_access_token(&path).as_deref(), Some("access-token"));
    }

    #[test]
    fn read_access_token_returns_none_for_missing_key() {
        let path = write_auth_file(r#"{"tokens":{"refresh_token":"refresh-token"}}"#);
        assert_eq!(read_access_token(&path), None);
    }

    #[test]
    fn read_access_token_returns_none_for_empty_string() {
        let path = write_auth_file(r#"{"tokens":{"access_token":""}}"#);
        assert_eq!(read_access_token(&path), None);
    }

    #[test]
    fn read_access_token_returns_none_for_malformed_json() {
        let path = write_auth_file(r#"{"tokens":{"access_token":"unterminated""#);
        assert_eq!(read_access_token(&path), None);
    }

    #[test]
    fn read_access_token_returns_none_for_api_key_mode() {
        let path = write_auth_file(r#"{"api_key":"sk-test"}"#);
        assert_eq!(read_access_token(&path), None);
    }
}
