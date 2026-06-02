#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use predicates::prelude::*;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use support::TestEnv;

fn wham_url(server: &MockServer) -> String {
    format!("{}/backend-api/wham/usage", server.uri())
}

fn add_account(env: &TestEnv, name: &str, account_id: &str) {
    env.seed_account(
        name,
        &format!(r#"{{"tokens":{{"access_token":"test-token","account_id":"{account_id}"}}}}"#),
    );
}

fn payload(five_hour: f64, weekly: f64) -> String {
    format!(
        r#"{{
    "rate_limit": {{
    "five_hour": {{ "percent_left": {five_hour}, "reset_time_ms": 1716393600000 }},
    "weekly": {{ "percent_left": {weekly}, "reset_time_ms": 1716998400000 }}
    }}
}}"#
    )
}

#[tokio::test]
async fn account_quota_text_and_json_modes_work() {
    let env = TestEnv::new();
    add_account(&env, "work", "acct-work");

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(payload(73.4, 87.1), "application/json"),
        )
        .mount(&server)
        .await;

    env.cmd()
        .env("CODEX_SESSION_WHAM_USAGE_URL", wham_url(&server))
        .args(["--account", "work", "account", "quota"])
        .assert()
        .success()
        .stdout(predicate::str::contains("work"))
        .stdout(predicate::str::contains("(active)"))
        .stdout(predicate::str::contains("5-hour"))
        .stdout(predicate::str::contains("TOTAL").not());

    let output = env
        .cmd()
        .env("CODEX_SESSION_WHAM_USAGE_URL", wham_url(&server))
        .args(["--account", "work", "account", "quota", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["entries"][0]["account"], "work");
    assert_eq!(value["entries"].as_array().unwrap().len(), 1);
    assert!(value["aggregate"].is_null());
}

#[tokio::test]
async fn account_quota_text_shows_full_for_near_empty_window() {
    let env = TestEnv::new();
    add_account(&env, "work", "acct-work");

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .respond_with(
            // A freshly-reset five-hour window comes back as used_percent: 1,
            // i.e. percent_left == 99.0; the human-facing text must read 100% left.
            ResponseTemplate::new(200).set_body_raw(payload(99.0, 65.0), "application/json"),
        )
        .mount(&server)
        .await;

    env.cmd()
        .env("CODEX_SESSION_WHAM_USAGE_URL", wham_url(&server))
        .args(["--account", "work", "account", "quota"])
        .assert()
        .success()
        .stdout(predicate::str::contains("100% left"))
        // The old behavior rendered this near-empty window as "99.0% left";
        // guard against that regression returning.
        .stdout(predicate::str::contains("99.0%").not());
}

#[tokio::test]
async fn account_quota_live_and_named_account_work() {
    let env = TestEnv::new();
    add_account(&env, "work", "acct-work");

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(payload(80.0, 90.0), "application/json"),
        )
        .mount(&server)
        .await;

    env.cmd()
        .env("CODEX_SESSION_WHAM_USAGE_URL", wham_url(&server))
        .args(["account", "quota", "--account", "work", "--format", "json"])
        .assert()
        .success();

    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn account_quota_all_orders_real_quota_before_api_key() {
    let env = TestEnv::new_empty();
    add_account(&env, "high", "acct-high");
    add_account(&env, "mid", "acct-mid");
    env.seed_account("scratch", r#"{"OPENAI_API_KEY":"sk-test"}"#);

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .and(header("ChatGPT-Account-Id", "acct-high"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(payload(90.0, 90.0), "application/json")
                .append_header("x-test", "ok"),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .and(header("ChatGPT-Account-Id", "acct-mid"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(payload(60.0, 60.0), "application/json"),
        )
        .expect(1)
        .mount(&server)
        .await;

    let output = env
        .cmd()
        .env("CODEX_SESSION_WHAM_USAGE_URL", wham_url(&server))
        .args(["account", "quota", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let items = value["entries"].as_array().unwrap();
    assert_eq!(items.len(), 3);
    assert_eq!(items[0]["account"], "high");
    assert_eq!(items[0]["mode"], "oauth");
    assert_eq!(items[1]["account"], "mid");
    assert_eq!(items[2]["mode"], "api-key");
    assert_eq!(value["aggregate"]["accounts-counted"], 2);
}

#[tokio::test]
async fn account_quota_all_flag_is_accepted_with_deprecation_warning() {
    let env = TestEnv::new_empty();
    add_account(&env, "demo", "acct-demo");

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(payload(50.0, 50.0), "application/json"),
        )
        .mount(&server)
        .await;

    env.cmd()
        .env("CODEX_SESSION_WHAM_USAGE_URL", wham_url(&server))
        .args(["account", "quota", "--all", "--format", "json"])
        .assert()
        .success()
        .stderr(predicate::str::contains("--all is deprecated"));
}

#[tokio::test]
async fn account_quota_default_shows_all_accounts_text() {
    let env = TestEnv::new_empty();
    add_account(&env, "alpha", "acct-alpha");
    add_account(&env, "beta", "acct-beta");

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .and(header("ChatGPT-Account-Id", "acct-alpha"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(payload(80.0, 95.0), "application/json"),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .and(header("ChatGPT-Account-Id", "acct-beta"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(payload(30.0, 70.0), "application/json"),
        )
        .mount(&server)
        .await;

    env.cmd()
        .env("CODEX_SESSION_WHAM_USAGE_URL", wham_url(&server))
        .args(["account", "quota"])
        .assert()
        .success()
        .stdout(predicate::str::contains("alpha"))
        .stdout(predicate::str::contains("beta"))
        .stdout(predicate::str::contains("5-hour"))
        .stdout(predicate::str::contains("Weekly"))
        .stdout(predicate::str::contains("TOTAL (avg across 2 accounts)"));
}

#[tokio::test]
async fn account_quota_multi_text_shows_total_panel() {
    let env = TestEnv::new_empty();
    add_account(&env, "alpha", "acct-alpha");
    add_account(&env, "beta", "acct-beta");

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .and(header("ChatGPT-Account-Id", "acct-alpha"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(payload(80.0, 95.0), "application/json"),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .and(header("ChatGPT-Account-Id", "acct-beta"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(payload(30.0, 70.0), "application/json"),
        )
        .mount(&server)
        .await;

    env.cmd()
        .env("CODEX_SESSION_WHAM_USAGE_URL", wham_url(&server))
        .args(["account", "quota"])
        .assert()
        .success()
        .stdout(predicate::str::contains("TOTAL (avg across 2 accounts)"))
        .stdout(predicate::str::contains("  5-hour      "))
        .stdout(predicate::str::contains("  Weekly      "));
}

#[tokio::test]
async fn account_quota_multi_json_wraps_entries_and_aggregate() {
    let env = TestEnv::new_empty();
    add_account(&env, "alpha", "acct-alpha");
    add_account(&env, "beta", "acct-beta");

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .and(header("ChatGPT-Account-Id", "acct-alpha"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(payload(80.0, 95.0), "application/json"),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .and(header("ChatGPT-Account-Id", "acct-beta"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(payload(30.0, 70.0), "application/json"),
        )
        .mount(&server)
        .await;

    let output = env
        .cmd()
        .env("CODEX_SESSION_WHAM_USAGE_URL", wham_url(&server))
        .args(["account", "quota", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["entries"].as_array().unwrap().len(), 2);
    assert_eq!(value["aggregate"]["accounts-counted"], 2);
    let mean = value["aggregate"]["five-hour"]["percent-left"]
        .as_f64()
        .unwrap();
    assert!((mean - 55.0).abs() < 1e-6);
}

#[tokio::test]
async fn account_quota_single_json_has_null_aggregate() {
    let env = TestEnv::new();
    add_account(&env, "work", "acct-work");

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(payload(73.4, 87.1), "application/json"),
        )
        .mount(&server)
        .await;

    let output = env
        .cmd()
        .env("CODEX_SESSION_WHAM_USAGE_URL", wham_url(&server))
        .args(["--account", "work", "account", "quota", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["entries"].as_array().unwrap().len(), 1);
    assert!(value["aggregate"].is_null());
}

#[tokio::test]
async fn account_quota_all_api_key_pool_has_no_aggregate() {
    let env = TestEnv::new_empty();
    env.seed_account("alpha", r#"{"OPENAI_API_KEY":"sk-alpha"}"#);
    env.seed_account("beta", r#"{"OPENAI_API_KEY":"sk-beta"}"#);

    let json_output = env
        .cmd()
        .args(["account", "quota", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&json_output).unwrap();
    assert!(value["aggregate"].is_null());

    env.cmd()
        .args(["account", "quota"])
        .assert()
        .success()
        .stdout(predicate::str::contains("TOTAL").not());
}

#[tokio::test]
async fn account_quota_aggregate_counts_only_oauth_entries() {
    let env = TestEnv::new_empty();
    add_account(&env, "alpha", "acct-alpha");
    add_account(&env, "beta", "acct-beta");
    add_account(&env, "broken", "acct-broken");

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .and(header("ChatGPT-Account-Id", "acct-alpha"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(payload(80.0, 95.0), "application/json"),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .and(header("ChatGPT-Account-Id", "acct-beta"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(payload(30.0, 70.0), "application/json"),
        )
        .mount(&server)
        .await;

    let output = env
        .cmd()
        .env("CODEX_SESSION_WHAM_USAGE_URL", wham_url(&server))
        .args(["account", "quota", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["entries"].as_array().unwrap().len(), 3);
    assert_eq!(value["aggregate"]["accounts-counted"], 2);
    let mean = value["aggregate"]["weekly"]["percent-left"]
        .as_f64()
        .unwrap();
    assert!((mean - 82.5).abs() < 1e-6);
}
