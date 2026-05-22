#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use support::TestEnv;

fn status_json(cmd: &mut assert_cmd::Command) -> serde_json::Value {
    let output = cmd
        .args(["config", "status", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&output).unwrap()
}

#[test]
fn group_flag_overrides_env() {
    let env = TestEnv::new();
    let value = status_json(
        env.cmd()
            .env("CODEX_SESSION_GROUP", "bar")
            .args(["--group", "foo"]),
    );
    assert_eq!(value["group-id"], "foo");
    assert_eq!(value["group-id-source"], "flag");
}

#[test]
fn group_env_overrides_derived_sources() {
    let env = TestEnv::new();
    let value = status_json(env.cmd().env("CODEX_SESSION_GROUP", "baz"));
    assert_eq!(value["group-id"], "baz");
    assert_eq!(value["group-id-source"], "env");
}

#[test]
fn invalid_group_flag_exits_64() {
    let env = TestEnv::new();
    env.cmd()
        .args(["--group", "BAD-VALUE!", "config", "status"])
        .assert()
        .code(64);
}

#[test]
fn invalid_group_env_exits_64() {
    let env = TestEnv::new();
    env.cmd()
        .env("CODEX_SESSION_GROUP", "BAD!")
        .args(["config", "status"])
        .assert()
        .code(64);
}
