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
    r#"{"tokens":{"access_token":"test-token","account_id":"acct-123","plan":"pro"}}"#
}

fn add_account(env: &TestEnv, name: &str) {
    env.write_native_auth("{\"token\":\"test\"}\n");
    env.cmd()
        .args(["account", "add", name, "--from-current"])
        .assert()
        .success();
    env.write_account_auth_seed(name, oauth_auth());
}

#[tokio::test]
async fn parses_rate_limit_shape() {
    let env = TestEnv::new();
    add_account(&env, "work");

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"{
    "rate_limit": {
    "five_hour": { "percent_left": 73.4, "reset_time_ms": 1716393600000 },
    "weekly": { "percent_left": 87.1, "reset_time_ms": 1716998400000 }
    }
}"#,
            "application/json",
        ))
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
    assert_eq!(value["weekly"]["percent-left"], 87.1);
}

#[tokio::test]
async fn parses_rate_limits_plural_shape() {
    let env = TestEnv::new();
    add_account(&env, "work");

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"{
    "rate_limits": {
    "five_hour": { "percent_left": 60.0, "reset_time_ms": 1716393600000 },
    "weekly": { "percent_left": 88.0, "reset_time_ms": 1716998400000 }
    }
}"#,
            "application/json",
        ))
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
    assert_eq!(value["five-hour"]["percent-left"], 60.0);
    assert_eq!(value["weekly"]["percent-left"], 88.0);
}

#[tokio::test]
async fn parses_primary_secondary_aliases() {
    let env = TestEnv::new();
    add_account(&env, "work");

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"{
    "rate_limit": {
    "primary_window": { "percent_left": 91.0, "reset_time_ms": 1716393600000 },
    "secondary_window": { "percent_left": 42.0, "reset_time_ms": 1716998400000 }
    }
}"#,
            "application/json",
        ))
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
    assert_eq!(value["five-hour"]["percent-left"], 91.0);
    assert_eq!(value["weekly"]["percent-left"], 42.0);
}

#[tokio::test]
async fn prefers_reset_time_ms_over_reset_at() {
    let env = TestEnv::new();
    add_account(&env, "work");

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"{
    "rate_limit": {
    "five_hour": {
        "percent_left": 80.0,
        "reset_time_ms": 1716393600000,
        "reset_at": "2030-01-01T00:00:00Z"
    },
    "weekly": {
        "percent_left": 80.0,
        "reset_time_ms": 1716998400000,
        "reset_at": "2031-01-01T00:00:00Z"
    }
    }
}"#,
            "application/json",
        ))
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
    assert_eq!(value["five-hour"]["reset-at-unix"], 1_716_393_600u64);
    assert_eq!(value["weekly"]["reset-at-unix"], 1_716_998_400u64);
}
