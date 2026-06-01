#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use std::process::Stdio;
use std::time::Duration;

use support::{TEST_AUTH, TestEnv};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn install_default_recipe_with_ping(env: &TestEnv) {
    std::fs::write(
        env.wrapper_user_config_path(),
        "[config-recipe]\ndefault = \"default\"\n",
    )
    .unwrap();
    env.install_config_recipe(
        "default",
        "config-layers:\n  - base\nprofile-files:\n  - ping\n",
        &[("base", "model = \"gpt-5.4\"\n")],
    );
    env.write_profile_file(
        "ping",
        "model = \"gpt-5.4-mini\"\nmodel_reasoning_effort = \"minimal\"\n",
    );
}

fn install_probe_child(env: &TestEnv) -> std::path::PathBuf {
    let script = [
        r#"#!/usr/bin/env bash
if [ "${"#,
        r#"1:-}" = "--version" ]; then
    printf '%s\n' 'codex 0.134.0'
    exit 0
fi
case "$(cat "$CODEX_HOME/auth.json")" in
    *access_token*) exit 0 ;;
    *) printf '%s\n' '401 Unauthorized' >&2; exit 1 ;;
esac
"#,
    ]
    .concat();
    let dir = env.make_fake_codex_in_dir("health-probe-child", &script);
    dir.join("codex")
}

const fn future_auth() -> &'static str {
    "{\"tokens\":{\"access_token\":\"eyJhbGciOiJub25lIn0.eyJleHAiOjQxMDI0NDQ4MDB9.\",\
\"refresh_token\":\"rt\",\"account_id\":\"a1\",\"plan\":\"pro\"}}"
}

#[tokio::test]
async fn health_fast_multi_account_json_contains_all_accounts() {
    let env = TestEnv::new_empty();
    env.seed_account("acct1", TEST_AUTH);
    env.seed_account("acct2", TEST_AUTH);

    let output = env
        .cmd()
        .args(["account", "health", "--fast", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let mut accounts = value
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["account"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    accounts.sort();

    assert_eq!(accounts, ["acct1", "acct2"]);
}

#[tokio::test]
async fn health_piped_text_output_has_no_spinner_artifacts() {
    let env = TestEnv::new_empty();
    env.seed_account("work", TEST_AUTH);

    let output = env
        .cmd()
        .args(["account", "health", "--fast", "--format", "text"])
        .assert()
        .success()
        .get_output()
        .clone();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(stdout.contains("RANK"));
    assert!(!stdout.contains('\u{1b}'), "stdout contains ANSI escapes");
    assert!(!stderr.contains('\u{1b}'), "stderr contains ANSI escapes");
    assert!(!stdout.contains('⠋'), "stdout contains spinner frames");
    assert!(!stderr.contains('⠋'), "stderr contains spinner frames");
    assert!(
        !stderr.contains("Checking account"),
        "stderr contains spinner narration"
    );
}

#[tokio::test]
async fn health_with_failing_account_cleans_up_spinner() {
    let env = TestEnv::new_empty();
    env.seed_account("good", future_auth());
    env.seed_account(
        "bad",
        r#"{"tokens":{"refresh_token":"rt","account_id":"acct-123"}}"#,
    );
    install_default_recipe_with_ping(&env);
    let child = install_probe_child(&env);

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(support::quota::default_payload(), "application/json"),
        )
        .mount(&server)
        .await;

    let output = env
        .cmd()
        .env("CODEX_SESSION_CHILD_BIN", child)
        .env(
            "CODEX_SESSION_WHAM_USAGE_URL",
            support::quota::wham_url(&server),
        )
        .args(["account", "health", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let entries = value.as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().any(|entry| entry["account"] == "good"));
    assert!(
        entries.iter().any(|entry| {
            entry["account"] == "bad" && entry["token"].as_str() == Some("invalid")
        })
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains('\u{1b}'), "stderr contains ANSI escapes");
    assert!(!stderr.contains('⠋'), "stderr contains spinner frame ⠋");
    assert!(!stderr.contains('⠙'), "stderr contains spinner frame ⠙");
}

#[tokio::test]
async fn health_killed_during_fetch_exits_cleanly() {
    let env = TestEnv::new_empty();
    env.seed_account("work", future_auth());
    install_default_recipe_with_ping(&env);
    let child_bin =
        env.make_fake_codex_with_version("codex 0.134.0", support::FakeCodexBehavior::Succeed);

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(support::quota::default_payload(), "application/json")
                .set_delay(Duration::from_secs(30)),
        )
        .mount(&server)
        .await;

    let mut child = env
        .std_cmd()
        .env("CODEX_SESSION_CHILD_BIN", child_bin)
        .env(
            "CODEX_SESSION_WHAM_USAGE_URL",
            support::quota::wham_url(&server),
        )
        .args(["account", "health", "--format", "json"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    // Bounded SIGINT test exception: give the process time to start and block
    // on the delayed quota fake before sending the signal.
    std::thread::sleep(Duration::from_secs(1));
    let pid = rustix::process::Pid::from_raw(child.id().cast_signed()).unwrap();
    rustix::process::kill_process(pid, rustix::process::Signal::INT).unwrap();
    let status = child.wait().unwrap();
    assert!(!status.success(), "health should not succeed after SIGINT");
}
