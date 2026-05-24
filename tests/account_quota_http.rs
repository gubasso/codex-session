#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

//! HTTP-behavior tests for the wham/usage quota reader: retry-on-5xx,
//! no-retry-on-4xx, stale-cache fallback when `--live` refresh fails, and
//! verification of the required request headers.

mod support;

use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

use support::TestEnv;

fn wham_url(server: &MockServer) -> String {
    format!("{}/backend-api/wham/usage", server.uri())
}

const fn oauth_auth() -> &'static str {
    r#"{"tokens":{"access_token":"test-token","account_id":"acct-123"}}"#
}

const fn payload() -> &'static str {
    r#"{
    "rate_limit": {
    "five_hour": { "percent_left": 73.4, "reset_time_ms": 1716393600000 },
    "weekly": { "percent_left": 87.1, "reset_time_ms": 1716998400000 }
    }
}"#
}

fn add_account(env: &TestEnv, name: &str) {
    env.write_native_auth("{\"token\":\"test\"}\n");
    env.cmd()
        .args(["account", "add", name, "--from-current"])
        .assert()
        .success();
    env.write_account_auth_seed(name, oauth_auth());
}

struct FirstFails {
    first_response: ResponseTemplate,
    rest_response: ResponseTemplate,
    counter: std::sync::atomic::AtomicU32,
}

impl Respond for FirstFails {
    fn respond(&self, _: &Request) -> ResponseTemplate {
        let n = self
            .counter
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if n == 0 {
            self.first_response.clone()
        } else {
            self.rest_response.clone()
        }
    }
}

/// 5xx then 200: the client must retry once and surface the 200.
#[tokio::test]
async fn retries_once_on_5xx_then_success() {
    let env = TestEnv::new();
    add_account(&env, "work");
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .respond_with(FirstFails {
            first_response: ResponseTemplate::new(503),
            rest_response: ResponseTemplate::new(200).set_body_raw(payload(), "application/json"),
            counter: std::sync::atomic::AtomicU32::new(0),
        })
        .mount(&server)
        .await;

    let output = env
        .cmd()
        .env("CODEX_SESSION_WHAM_USAGE_URL", wham_url(&server))
        .args([
            "account",
            "quota",
            "--account",
            "work",
            "--live",
            "--format",
            "json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["mode"], "oauth");
    assert_eq!(value["five-hour"]["percent-left"], 73.4);
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

/// 5xx twice: client surfaces an error (exit code 69, `EX_UNAVAILABLE`).
#[tokio::test]
async fn second_5xx_surfaces_http_status_error() {
    let env = TestEnv::new();
    add_account(&env, "work");
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    env.cmd()
        .env("CODEX_SESSION_WHAM_USAGE_URL", wham_url(&server))
        .args(["account", "quota", "--account", "work", "--live"])
        .assert()
        .code(69);

    // Both attempts went out: original + one retry.
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

/// 401 must not be retried.
#[tokio::test]
async fn no_retry_on_4xx() {
    let env = TestEnv::new();
    add_account(&env, "work");
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    env.cmd()
        .env("CODEX_SESSION_WHAM_USAGE_URL", wham_url(&server))
        .args(["account", "quota", "--account", "work", "--live"])
        .assert()
        .code(69);

    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

/// `--live` with a preseeded cache and a failing server must return
/// `Stale(prev)` rather than erroring.
#[tokio::test]
async fn live_falls_back_to_stale_cache_on_refresh_failure() {
    let env = TestEnv::new();
    add_account(&env, "work");

    // Seed an "old" cache file with quota=ok body but a stale `fetched_at`
    // timestamp so the freshness check would force a refresh.
    let cache = r#"{
    "fetched_at_unix": 0,
    "ttl_secs": 30,
    "body": {
    "kind": "ok",
    "five_hour": { "percent_left": 22.2, "reset_at_unix": 0 },
    "weekly":    { "percent_left": 33.3, "reset_at_unix": 0 }
    }
}"#;
    env.write_quota_cache("work", cache);

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let output = env
        .cmd()
        .env("CODEX_SESSION_WHAM_USAGE_URL", wham_url(&server))
        .args([
            "account",
            "quota",
            "--account",
            "work",
            "--live",
            "--format",
            "json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["mode"], "oauth");
    assert_eq!(value["stale"], true);
    assert_eq!(value["five-hour"]["percent-left"], 22.2);
    assert_eq!(value["weekly"]["percent-left"], 33.3);
}

/// The HTTP request must carry the exact header set documented in
/// `06-quota-protocol.md`.
#[tokio::test]
async fn required_headers_are_sent() {
    let env = TestEnv::new();
    add_account(&env, "work");
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .and(header("authorization", "Bearer test-token"))
        .and(header("chatgpt-account-id", "acct-123"))
        .and(header("accept", "application/json"))
        .and(header("origin", "https://chatgpt.com"))
        .and(header("referer", "https://chatgpt.com/"))
        .and(header("user-agent", "Mozilla/5.0"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(payload(), "application/json"))
        .mount(&server)
        .await;

    env.cmd()
        .env("CODEX_SESSION_WHAM_USAGE_URL", wham_url(&server))
        .args([
            "account",
            "quota",
            "--account",
            "work",
            "--live",
            "--format",
            "json",
        ])
        .assert()
        .success();

    // The mock matches on all required headers; missing any would fail the match
    // and the request count below would be zero.
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}
