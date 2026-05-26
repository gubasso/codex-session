#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use support::TestEnv;

fn wham_url(server: &MockServer) -> String {
    format!("{}/backend-api/wham/usage", server.uri())
}

const fn oauth_auth() -> &'static str {
    r#"{"tokens":{"access_token":"test-token","account_id":"acct-123"}}"#
}

fn add_account(env: &TestEnv, name: &str) {
    env.seed_account(name, oauth_auth());
}

const fn payload() -> &'static str {
    r#"{
    "rate_limit": {
    "five_hour": { "percent_left": 73.4, "reset_time_ms": 1716393600000 },
    "weekly": { "percent_left": 87.1, "reset_time_ms": 1716998400000 }
    }
}"#
}

#[tokio::test]
async fn every_invocation_fetches_live() {
    let env = TestEnv::new();
    add_account(&env, "work");
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(payload(), "application/json"))
        .mount(&server)
        .await;

    for _ in 0..2 {
        env.cmd()
            .env("CODEX_SESSION_WHAM_USAGE_URL", wham_url(&server))
            .args(["account", "quota", "--account", "work", "--format", "json"])
            .assert()
            .success();
    }

    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn malformed_cache_is_overwritten_on_next_fetch() {
    let env = TestEnv::new();
    add_account(&env, "work");
    env.write_quota_cache("work", "{not-json");

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(payload(), "application/json"))
        .mount(&server)
        .await;

    env.cmd()
        .env("CODEX_SESSION_WHAM_USAGE_URL", wham_url(&server))
        .args(["account", "quota", "--account", "work", "--format", "json"])
        .assert()
        .success();

    let bytes = std::fs::read(env.quota_cache_path("work")).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["body"]["kind"], "ok");
}

#[tokio::test]
async fn cache_write_leaves_no_temp_files() {
    let env = TestEnv::new();
    add_account(&env, "work");

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(payload(), "application/json"))
        .mount(&server)
        .await;

    env.cmd()
        .env("CODEX_SESSION_WHAM_USAGE_URL", wham_url(&server))
        .args(["account", "quota", "--account", "work", "--format", "json"])
        .assert()
        .success();

    let quota_dir = env.state_session_root().join("cache/quota");
    let leftovers: Vec<_> = std::fs::read_dir(quota_dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
        .filter(|name| name.contains(".tmp"))
        .collect();
    assert!(leftovers.is_empty());
}
