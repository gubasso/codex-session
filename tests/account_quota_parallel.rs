#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use std::time::Duration;

use support::TestEnv;
use support::quota::{add_oauth_account, default_payload, delayed_quota_responder, wham_url};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn quota_multi_account_requests_start_concurrently() {
    let env = TestEnv::new_empty();
    for account in ["acct1", "acct2", "acct3", "acct4"] {
        add_oauth_account(&env, account);
    }
    let server = MockServer::start().await;
    let (recorder, responder) = delayed_quota_responder(Duration::from_secs(2));

    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .respond_with(responder)
        .mount(&server)
        .await;

    let output = env
        .cmd()
        .env("CODEX_SESSION_WHAM_USAGE_URL", wham_url(&server))
        .args(["account", "quota", "--format", "json"])
        .timeout(Duration::from_secs(10))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["entries"].as_array().unwrap().len(), 4);
    assert_eq!(server.received_requests().await.unwrap().len(), 4);

    // Concurrency is proven by the request-arrival window (jitter-resistant),
    // not by a wall-clock elapsed bound: under sequential execution the four
    // 2s-delayed requests would arrive ~2s apart, so a sub-1s spread across all
    // four arrivals can only happen if they were dispatched concurrently. The
    // `.timeout(10s)` above still guards against a hang.
    let instants = recorder.instants();
    assert_eq!(instants.len(), 4);
    let window = instants
        .last()
        .unwrap()
        .duration_since(*instants.first().unwrap());
    assert!(
        window < Duration::from_secs(1),
        "expected requests to arrive concurrently, spread was {window:?}"
    );
}

#[tokio::test]
async fn quota_json_output_has_no_spinner_artifacts() {
    let env = TestEnv::new_empty();
    add_oauth_account(&env, "work");
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(default_payload(), "application/json"),
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
        .clone();

    let _: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains('\u{1b}'), "stderr contains ANSI escapes");
    assert!(
        !stderr.contains("Fetching quota"),
        "stderr contains spinner narration"
    );
    for frame in ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'] {
        assert!(
            !stderr.contains(frame),
            "stderr contains spinner frame {frame}"
        );
    }
}
