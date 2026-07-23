#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use predicates::prelude::*;
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

#[tokio::test]
async fn parses_iso_reset_at_and_tolerates_unknown_keys() {
    let env = TestEnv::new();
    add_account(&env, "work");

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"{
    "unexpected": "ok",
    "rate_limit": {
    "five_hour": {
        "percent_left": 75.0,
        "reset_at": "2026-05-22T18:00:00Z",
        "extra": true
    },
    "weekly": {
        "percent_left": 65.0,
        "reset_at": "2026-05-26T00:00:00Z",
        "another": 42
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
        .args(["account", "quota", "--account", "work", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert!(value["aggregate"].is_null());
    assert_eq!(value["entries"][0]["five-hour"]["percent-left"], 75.0);
    assert_eq!(value["entries"][0]["weekly"]["percent-left"], 65.0);
    assert!(
        value["entries"][0]["five-hour"]["reset-at-unix"]
            .as_u64()
            .unwrap()
            > 0
    );
}

#[tokio::test]
async fn empty_body_is_missing_rate_limit() {
    let env = TestEnv::new();
    add_account(&env, "work");

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .respond_with(ResponseTemplate::new(200).set_body_string(""))
        .mount(&server)
        .await;

    env.cmd()
        .env("CODEX_SESSION_WHAM_USAGE_URL", wham_url(&server))
        .args(["account", "quota", "--account", "work"])
        .assert()
        .failure()
        .code(65)
        .stderr(predicate::str::contains("missing rate_limit"));
}

#[tokio::test]
async fn missing_five_hour_window_is_tolerated() {
    // A response with only the weekly window is valid: the command succeeds
    // with the five-hour window absent.
    let env = TestEnv::new();
    add_account(&env, "work");

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"{
    "rate_limit": {
    "weekly": { "percent_left": 65.0, "reset_time_ms": 1716998400000 }
    }
}"#,
            "application/json",
        ))
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
    assert!(value["entries"][0]["five-hour"].is_null());
    let weekly_pct = value["entries"][0]["weekly"]["percent-left"]
        .as_f64()
        .unwrap();
    assert!((weekly_pct - 65.0).abs() < 0.01);
}

#[tokio::test]
async fn missing_weekly_window_is_tolerated() {
    // Symmetric case: only a short (five-hour) window present — succeed with the
    // weekly window absent.
    let env = TestEnv::new();
    add_account(&env, "work");

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"{
    "rate_limit": {
    "five_hour": { "percent_left": 65.0, "reset_time_ms": 1716393600000 }
    }
}"#,
            "application/json",
        ))
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
    assert!(value["entries"][0]["weekly"].is_null());
    let five_hour_pct = value["entries"][0]["five-hour"]["percent-left"]
        .as_f64()
        .unwrap();
    assert!((five_hour_pct - 65.0).abs() < 0.01);
}

#[tokio::test]
async fn new_shape_tolerates_extra_fields() {
    let env = TestEnv::new();
    add_account(&env, "work");

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"{
    "rate_limits": {
    "primary": {
        "usedPercent": 30.0,
        "resetsAt": 1716393600,
        "windowDurationMins": 300,
        "unknownField": true
    },
    "secondary": {
        "usedPercent": 10.0,
        "resetsAt": 1716998400,
        "windowDurationMins": 10080,
        "anotherExtra": 42
    },
    "models": { "gpt-4o": {} },
    "code_review": {}
    }
}"#,
            "application/json",
        ))
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
    let five_hour_pct = value["entries"][0]["five-hour"]["percent-left"]
        .as_f64()
        .unwrap();
    assert!((five_hour_pct - 70.0).abs() < 0.01);
}

#[tokio::test]
async fn only_secondary_window_is_tolerated() {
    // Only the `secondary` (weekly) alias present, no five-hour: succeed with
    // the weekly window and an absent five-hour window.
    let env = TestEnv::new();
    add_account(&env, "work");

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"{
    "rate_limits": {
    "secondary": { "usedPercent": 10.0, "resetsAt": 1716998400 }
    }
}"#,
            "application/json",
        ))
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
    assert!(value["entries"][0]["five-hour"].is_null());
    let weekly_pct = value["entries"][0]["weekly"]["percent-left"]
        .as_f64()
        .unwrap();
    assert!((weekly_pct - 90.0).abs() < 0.01);
}

#[tokio::test]
async fn no_windows_present_is_parse_error() {
    // A `rate_limit` object with no usable window (e.g. `secondary_window: null`
    // and no primary) is a genuine schema break: fail loudly.
    let env = TestEnv::new();
    add_account(&env, "work");

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"{
    "rate_limit": { "allowed": true, "secondary_window": null }
}"#,
            "application/json",
        ))
        .mount(&server)
        .await;

    env.cmd()
        .env("CODEX_SESSION_WHAM_USAGE_URL", wham_url(&server))
        .args(["account", "quota", "--account", "work"])
        .assert()
        .failure()
        .code(65)
        .stderr(predicate::str::contains("no rate-limit windows present"));
}
