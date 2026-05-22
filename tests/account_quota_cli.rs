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
    env.cmd().args(["account", "add", name]).assert().success();
    env.write_account_auth_seed(
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
    env.cmd()
        .args(["account", "use", "work"])
        .assert()
        .success();

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
        .args(["account", "quota"])
        .assert()
        .success()
        .stdout(predicate::str::contains("account: work (active)"))
        .stdout(predicate::str::contains("five-hour:"));

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
    assert_eq!(value["account"], "work");
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

    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn account_quota_all_orders_real_quota_before_api_key() {
    let env = TestEnv::new();
    add_account(&env, "high", "acct-high");
    add_account(&env, "mid", "acct-mid");
    env.cmd()
        .args(["account", "add", "scratch"])
        .assert()
        .success();
    env.write_account_auth_seed("scratch", r#"{"OPENAI_API_KEY":"sk-test"}"#);

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
        .args(["account", "quota", "--all", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let items = value.as_array().unwrap();
    assert_eq!(items.len(), 3);
    assert_eq!(items[0]["account"], "high");
    assert_eq!(items[0]["mode"], "oauth");
    assert_eq!(items[1]["account"], "mid");
    assert_eq!(items[2]["mode"], "api-key");
}
