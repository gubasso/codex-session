#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use predicates::prelude::*;
use support::TestEnv;

const TEST_AUTH: &str = r#"{"tokens":{
    "access_token":"eyJhbGciOiJub25lIn0.eyJleHAiOjE3MDAwMDAwMDB9.",
    "account_id":"acct-123","plan":"pro"}}"#;

#[test]
fn health_rejects_account_auto() {
    let env = TestEnv::new();
    env.cmd()
        .args(["--account", "auto", "account", "health"])
        .assert()
        .failure()
        .code(64)
        .stderr(predicate::str::contains(
            "--account auto is not supported for account health",
        ));
}

#[test]
fn health_requires_ping_profile_when_not_fast() {
    let env = TestEnv::new_empty();
    env.seed_account("work", TEST_AUTH);
    std::fs::write(
        env.wrapper_user_config_path(),
        "[config-recipe]\ndefault = \"test\"\n",
    )
    .unwrap();
    env.install_config_recipe(
        "test",
        "config-layers:\n  - base\n",
        &[("base", "model = \"gpt-4.1-nano\"\n")],
    );

    env.cmd()
        .args(["account", "health"])
        .assert()
        .failure()
        .code(78)
        .stderr(predicate::str::contains("[profiles.ping]"));
}

#[test]
fn health_fast_skips_ping_profile_validation() {
    let env = TestEnv::new_empty();
    env.seed_account("work", TEST_AUTH);

    env.cmd()
        .args(["account", "health", "--fast", "--format", "json"])
        .assert()
        .success();
}

#[test]
fn health_fast_json_token_is_bare_unknown() {
    let env = TestEnv::new_empty();
    env.seed_account("work", TEST_AUTH);

    let out = env
        .cmd()
        .args(["account", "health", "--fast", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let arr = value.as_array().unwrap();
    assert_eq!(arr[0]["token"], "unknown");
}

#[test]
fn health_fast_json_uses_cache_when_present() {
    let env = TestEnv::new_empty();
    env.seed_account("work", TEST_AUTH);

    let cache = env
        .state_session_root()
        .join("cache")
        .join("quota")
        .join("work.json");
    std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
    std::fs::write(
        &cache,
        r#"{
    "fetched_at_unix": 1700000000,
    "ttl_secs": 30,
    "body": {
        "kind": "ok",
        "five_hour": {"percent_left": 80.0, "reset_at_unix": 1710000000},
        "weekly": {"percent_left": 70.0, "reset_at_unix": 1710500000}
    }
}"#,
    )
    .unwrap();

    let out = env
        .cmd()
        .args(["account", "health", "--fast", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let arr = value.as_array().unwrap();
    assert_eq!(arr[0]["account"], "work");
    assert_eq!(arr[0]["status"], "cache only");
    assert!(arr[0]["score"].is_number());
    assert_eq!(arr[0]["fetched-at-unix"], 1_700_000_000_u64);
}
