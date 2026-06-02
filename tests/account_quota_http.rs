#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

//! HTTP-behavior tests for the wham/usage quota reader: retry-on-5xx,
//! no-retry-on-4xx, stale-cache fallback when live refresh fails, and
//! verification of the required request headers.

mod support;

use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

use support::TestEnv;
use support::quota::{add_oauth_account as add_account, default_payload as payload, wham_url};

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
        .args(["account", "quota", "--account", "work", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert!(value["aggregate"].is_null());
    assert_eq!(value["entries"][0]["mode"], "oauth");
    assert_eq!(value["entries"][0]["five-hour"]["percent-left"], 73.4);
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
        .args(["account", "quota", "--account", "work"])
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
        .args(["account", "quota", "--account", "work"])
        .assert()
        .code(69);

    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

/// A failing server must surface an error even when a stale cache exists.
#[tokio::test]
async fn fetch_failure_surfaces_error_despite_cached_data() {
    let env = TestEnv::new();
    add_account(&env, "work");

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

    env.cmd()
        .env("CODEX_SESSION_WHAM_USAGE_URL", wham_url(&server))
        .args(["account", "quota", "--account", "work"])
        .assert()
        .code(69);
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
        .args(["account", "quota", "--account", "work", "--format", "json"])
        .assert()
        .success();

    // The mock matches on all required headers; missing any would fail the match
    // and the request count below would be zero.
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}
