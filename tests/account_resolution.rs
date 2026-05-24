#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use support::TestEnv;

fn write_user_config(env: &TestEnv, body: &str) {
    std::fs::write(env.wrapper_user_config_path(), body).unwrap();
}

#[test]
fn account_flag_overrides_env_and_lru() {
    let env = TestEnv::new();
    env.write_native_auth("{\"token\":\"test\"}\n");
    env.cmd()
        .args(["account", "add", "flag", "--from-current"])
        .assert()
        .success();
    env.cmd()
        .args(["account", "add", "env", "--from-current"])
        .assert()
        .success();
    env.cmd()
        .args(["account", "add", "lru", "--from-current"])
        .assert()
        .success();
    std::fs::write(env.last_account_path(), "lru").unwrap();
    let output = env
        .cmd()
        .env("CODEX_SESSION_ACCOUNT", "env")
        .args([
            "--account",
            "flag",
            "account",
            "current",
            "--format",
            "json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["name"], "flag");
    assert_eq!(value["source"], "flag");
}

#[test]
fn account_env_overrides_lru_and_config() {
    let env = TestEnv::new();
    env.write_native_auth("{\"token\":\"test\"}\n");
    env.cmd()
        .args(["account", "add", "env", "--from-current"])
        .assert()
        .success();
    env.cmd()
        .args(["account", "add", "lru", "--from-current"])
        .assert()
        .success();
    std::fs::write(env.last_account_path(), "lru").unwrap();
    write_user_config(
        &env,
        "[account]\npinned = \"pinned\"\ndefault = \"default\"\n",
    );
    let output = env
        .cmd()
        .env("CODEX_SESSION_ACCOUNT", "env")
        .args(["account", "current", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["name"], "env");
    assert_eq!(value["source"], "env");
}

#[test]
fn account_lru_overrides_config_pinned() {
    let env = TestEnv::new();
    env.write_native_auth("{\"token\":\"test\"}\n");
    env.cmd()
        .args(["account", "add", "lru", "--from-current"])
        .assert()
        .success();
    std::fs::write(env.last_account_path(), "lru").unwrap();
    write_user_config(&env, "[account]\npinned = \"pinned\"\n");
    let output = env
        .cmd()
        .args(["account", "current", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["name"], "lru");
    assert_eq!(value["source"], "lru");
}

#[test]
fn account_auto_invokes_selector() {
    let env = TestEnv::new();
    env.write_native_auth("{\"token\":\"test\"}\n");
    env.cmd()
        .args(["account", "add", "auto", "--from-current"])
        .assert()
        .success();
    env.make_fake_codex();
    let output = env
        .cmd()
        .args(["--account", "auto", "exec"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(output).unwrap();
    assert!(stdout.is_empty());
}

#[test]
fn account_invalid_env_exits_64() {
    let env = TestEnv::new();
    env.cmd()
        .env("CODEX_SESSION_ACCOUNT", "BAD!")
        .args(["account", "current"])
        .assert()
        .failure()
        .code(64);
}
