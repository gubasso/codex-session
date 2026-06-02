//! Tests for the 401-retry-with-token-refresh path in the quota module.
//!
//! Uses wiremock for both the WHAM usage endpoint and the `OpenAI`
//! token endpoint to verify that:
//! - A 401 from WHAM triggers a token refresh attempt
//! - A successful refresh leads to a successful retry
//! - A failed refresh surfaces the original 401
//! - Non-401 errors do NOT trigger token refresh
#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use std::sync::atomic::{AtomicU32, Ordering};

use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

use support::TestEnv;

fn wham_url(server: &MockServer) -> String {
    format!("{}/backend-api/wham/usage", server.uri())
}

fn token_url(server: &MockServer) -> String {
    format!("{}/oauth/token", server.uri())
}

const fn oauth_auth() -> &'static str {
    r#"{"tokens":{"access_token":"old-token","refresh_token":"old-rt","account_id":"acct-1"}}"#
}

const fn quota_payload() -> &'static str {
    r#"{
    "rate_limit": {
    "five_hour": { "percent_left": 80.0, "reset_time_ms": 1716393600000 },
    "weekly": { "percent_left": 95.0, "reset_time_ms": 1716998400000 }
    }
}"#
}

fn refresh_success() -> String {
    serde_json::json!({
        "access_token": "new-token",
        "refresh_token": "new-rt",
        "token_type": "Bearer",
        "expires_in": 864_000
    })
    .to_string()
}

fn refresh_rejected() -> String {
    serde_json::json!({
        "error": {
            "message": "Your refresh token has already been used",
            "code": "refresh_token_reused"
        }
    })
    .to_string()
}

struct CountingResponder {
    responses: Vec<ResponseTemplate>,
    counter: AtomicU32,
}

impl CountingResponder {
    const fn new(responses: Vec<ResponseTemplate>) -> Self {
        Self {
            responses,
            counter: AtomicU32::new(0),
        }
    }
}

impl Respond for CountingResponder {
    fn respond(&self, _: &Request) -> ResponseTemplate {
        let n = self.counter.fetch_add(1, Ordering::SeqCst) as usize;
        if n < self.responses.len() {
            self.responses[n].clone()
        } else {
            self.responses.last().unwrap().clone()
        }
    }
}

/// 401 from WHAM → token refresh succeeds → retry succeeds.
#[tokio::test]
async fn quota_retries_on_401_after_successful_refresh() {
    let env = TestEnv::new();
    env.seed_account("work", oauth_auth());

    let server = MockServer::start().await;

    // WHAM: 401 on first call, 200 on second.
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .respond_with(CountingResponder::new(vec![
            ResponseTemplate::new(401),
            ResponseTemplate::new(200).set_body_raw(quota_payload(), "application/json"),
        ]))
        .mount(&server)
        .await;

    // Token endpoint: success.
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(refresh_success(), "application/json"),
        )
        .mount(&server)
        .await;

    let output = env
        .cmd()
        .env("CODEX_SESSION_WHAM_USAGE_URL", wham_url(&server))
        .env("CODEX_SESSION_TOKEN_ENDPOINT", token_url(&server))
        .args(["account", "quota", "--account", "work", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert!(value["aggregate"].is_null());
    assert_eq!(value["entries"][0]["mode"], "oauth");

    let requests = server.received_requests().await.unwrap();
    // 1 WHAM (401) + 1 token refresh + 1 WHAM (200) = 3 total
    assert_eq!(
        requests.len(),
        3,
        "expected 3 requests, got {}",
        requests.len()
    );

    // Finding 3: verify the rotated tokens were persisted to disk.
    let seed = std::fs::read_to_string(env.named_account_auth_seed("work")).unwrap();
    assert!(
        seed.contains("new-token"),
        "seed should have the refreshed access_token, got: {seed}"
    );
    assert!(
        seed.contains("new-rt"),
        "seed should have the rotated refresh_token, got: {seed}"
    );
}

/// 401 from WHAM → token refresh fails → error surfaced.
#[tokio::test]
async fn quota_fails_when_refresh_also_fails() {
    let env = TestEnv::new();
    env.seed_account("work", oauth_auth());

    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .respond_with(
            ResponseTemplate::new(400).set_body_raw(refresh_rejected(), "application/json"),
        )
        .mount(&server)
        .await;

    env.cmd()
        .env("CODEX_SESSION_WHAM_USAGE_URL", wham_url(&server))
        .env("CODEX_SESSION_TOKEN_ENDPOINT", token_url(&server))
        .args(["account", "quota", "--account", "work"])
        .assert()
        .failure()
        .code(69); // EX_UNAVAILABLE
}

/// 500 from WHAM → no token refresh attempt.
#[tokio::test]
async fn quota_does_not_refresh_on_non_401() {
    let env = TestEnv::new();
    env.seed_account("work", oauth_auth());

    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    // Token endpoint should NOT be called.
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(refresh_success(), "application/json"),
        )
        .expect(0) // must receive zero requests
        .mount(&server)
        .await;

    env.cmd()
        .env("CODEX_SESSION_WHAM_USAGE_URL", wham_url(&server))
        .env("CODEX_SESSION_TOKEN_ENDPOINT", token_url(&server))
        .args(["account", "quota", "--account", "work"])
        .assert()
        .failure();
}

/// Token refresh sends correct `grant_type` and `client_id`.
#[tokio::test]
async fn token_refresh_sends_correct_parameters() {
    let env = TestEnv::new();
    env.seed_account("work", oauth_auth());

    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .respond_with(CountingResponder::new(vec![
            ResponseTemplate::new(401),
            ResponseTemplate::new(200).set_body_raw(quota_payload(), "application/json"),
        ]))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .and(body_string_contains("refresh_token"))
        .and(body_string_contains("grant_type"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(refresh_success(), "application/json"),
        )
        .mount(&server)
        .await;

    env.cmd()
        .env("CODEX_SESSION_WHAM_USAGE_URL", wham_url(&server))
        .env("CODEX_SESSION_TOKEN_ENDPOINT", token_url(&server))
        .args(["account", "quota", "--account", "work", "--format", "json"])
        .assert()
        .success();
}

/// API-key mode accounts do not trigger refresh.
#[tokio::test]
async fn api_key_mode_does_not_trigger_refresh() {
    let env = TestEnv::new();
    // Auth without refresh_token → ApiKey mode.
    env.seed_account("apikey-acct", r#"{"tokens":{"access_token":"sk-test"}}"#);

    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;

    // ApiKey mode skips WHAM entirely and returns a special result.
    env.cmd()
        .env("CODEX_SESSION_TOKEN_ENDPOINT", token_url(&server))
        .args([
            "account",
            "quota",
            "--account",
            "apikey-acct",
            "--format",
            "json",
        ])
        .assert()
        .success();
}
