#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use support::{TEST_AUTH, TestEnv};

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
