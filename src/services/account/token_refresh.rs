//! OAuth token refresh against the `OpenAI` auth endpoint.
//!
//! `OpenAI` uses single-use refresh tokens (RFC 6749 rotation): each
//! refresh returns a new `access_token` AND a new `refresh_token`.
//! The old `refresh_token` is permanently invalidated.
#![allow(clippy::missing_errors_doc, clippy::result_large_err)]

use std::time::Duration;

use camino::Utf8Path;
use serde_json::Value;

const DEFAULT_TOKEN_ENDPOINT: &str = "https://auth.openai.com/oauth/token";
const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct RefreshResult {
    pub(crate) access_token: String,
    pub(crate) refresh_token: String,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum RefreshError {
    #[error("no refresh_token in auth file")]
    NoRefreshToken,
    #[error("refresh request failed: {0}")]
    Network(String),
    #[error("refresh rejected: {0}")]
    Rejected(String),
    #[error("unexpected response shape")]
    BadResponse,
    #[error("{0}")]
    Io(String),
}

fn token_endpoint() -> String {
    std::env::var("CODEX_SESSION_TOKEN_ENDPOINT")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| DEFAULT_TOKEN_ENDPOINT.to_owned())
}

pub(crate) fn refresh_token(auth_path: &Utf8Path) -> Result<RefreshResult, RefreshError> {
    let bytes = crate::services::auth::secure_file_read(auth_path)
        .map_err(|err| RefreshError::Io(err.to_string()))?;
    let mut doc: Value =
        serde_json::from_slice(&bytes).map_err(|err| RefreshError::Io(err.to_string()))?;

    let old_rt = doc
        .get("tokens")
        .and_then(|t| t.get("refresh_token"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or(RefreshError::NoRefreshToken)?
        .to_owned();

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|err| RefreshError::Network(err.to_string()))?;

    let resp = client
        .post(token_endpoint())
        .json(&serde_json::json!({
            "grant_type": "refresh_token",
            "client_id": CLIENT_ID,
            "refresh_token": old_rt,
        }))
        .send()
        .map_err(|err| RefreshError::Network(err.to_string()))?;

    if !resp.status().is_success() {
        let body: Value = resp.json().unwrap_or_default();
        let msg = body
            .get("error")
            .and_then(|e| e.get("message").or(Some(e)))
            .and_then(Value::as_str)
            .or_else(|| body.get("error_description").and_then(Value::as_str))
            .unwrap_or("unknown")
            .to_owned();
        return Err(RefreshError::Rejected(msg));
    }

    let body: Value = resp
        .json()
        .map_err(|err| RefreshError::Network(err.to_string()))?;

    let fresh_access = body
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or(RefreshError::BadResponse)?
        .to_owned();

    let fresh_refresh = body
        .get("refresh_token")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or(RefreshError::BadResponse)?
        .to_owned();

    if let Some(tokens) = doc.get_mut("tokens").and_then(Value::as_object_mut) {
        tokens.insert(
            "access_token".to_owned(),
            Value::String(fresh_access.clone()),
        );
        tokens.insert(
            "refresh_token".to_owned(),
            Value::String(fresh_refresh.clone()),
        );
    }

    let updated =
        serde_json::to_vec_pretty(&doc).map_err(|err| RefreshError::Io(err.to_string()))?;
    crate::adapters::fs::atomic_write(auth_path, &updated)
        .map_err(|err| RefreshError::Io(err.to_string()))?;

    tracing::info!(op = "token_refresh", outcome = "ok");
    Ok(RefreshResult {
        access_token: fresh_access,
        refresh_token: fresh_refresh,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn extracts_refresh_token_from_valid_json() {
        let doc: Value = serde_json::from_str(
            r#"{"tokens":{"access_token":"at","refresh_token":"rt","account_id":"a"}}"#,
        )
        .unwrap();
        let rt = doc
            .get("tokens")
            .and_then(|t| t.get("refresh_token"))
            .and_then(Value::as_str)
            .unwrap();
        assert_eq!(rt, "rt");
    }

    #[test]
    fn no_refresh_token_when_api_key_mode() {
        let doc: Value =
            serde_json::from_str(r#"{"tokens":{"access_token":"at","account_id":"a"}}"#).unwrap();
        let rt = doc
            .get("tokens")
            .and_then(|t| t.get("refresh_token"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty());
        assert!(rt.is_none());
    }
}
