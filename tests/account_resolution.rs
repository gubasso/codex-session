#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use support::TestEnv;

fn write_user_config(env: &TestEnv, body: &str) {
    std::fs::write(env.wrapper_user_config_path(), body).unwrap();
}

#[test]
fn account_flag_overrides_env_and_auto_display() {
    let env = TestEnv::new();
    env.seed_account("flag", "{\"token\":\"test\"}\n");
    env.seed_account("env", "{\"token\":\"test\"}\n");
    env.seed_account("last", "{\"token\":\"test\"}\n");
    std::fs::write(env.last_account_path(), "last").unwrap();
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
fn account_env_overrides_auto_display_and_config() {
    let env = TestEnv::new();
    env.seed_account("env", "{\"token\":\"test\"}\n");
    env.seed_account("last", "{\"token\":\"test\"}\n");
    std::fs::write(env.last_account_path(), "last").unwrap();
    write_user_config(&env, "[account]\npinned = \"BAD!\"\n");
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
fn account_current_defaults_to_auto_displaying_last_selected() {
    let env = TestEnv::new();
    env.seed_account("last", "{\"token\":\"test\"}\n");
    std::fs::write(env.last_account_path(), "last").unwrap();
    write_user_config(&env, "[account]\npinned = \"BAD!\"\n");
    let output = env
        .cmd()
        .args(["account", "current", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["name"], "last");
    assert_eq!(value["source"], "auto");
}

#[test]
fn exec_without_account_flag_auto_selects_account() {
    let env = TestEnv::new();
    env.make_fake_codex();
    env.cmd().args(["exec"]).assert().success();
    let meta: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(env.session_dir().join("session-meta.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(meta["account"], "default");
    assert_eq!(meta["account-source"], "auto");
}

#[test]
fn account_auto_invokes_selector() {
    let env = TestEnv::new();
    env.seed_account("auto", "{\"token\":\"test\"}\n");
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
