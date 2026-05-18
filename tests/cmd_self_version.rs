#![allow(clippy::unwrap_used)]

pub mod support;

use insta::{assert_json_snapshot, assert_snapshot};
use support::TestEnv;

#[test]
fn self_version_text_snapshot() {
    let env = TestEnv::new();
    env.make_fake_codex_printing_stdout("codex 1.2.3\n");

    let output = env
        .cmd()
        .args(["self", "version"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = env.normalize_text(&String::from_utf8(output).unwrap());
    assert_snapshot!("self_version_text", stdout);
}

#[test]
fn self_version_does_not_recurse_when_child_bin_points_at_wrapper() {
    // Misconfig guard: if CODEX_SESSION_CHILD_BIN points at the wrapper
    // itself, `self version` must NOT spawn `<wrapper> --version` (which
    // would re-enter pass-through and exec-loop). Instead it must skip the
    // probe and report `child-version: null` / `(unknown)`.
    let env = TestEnv::new();
    let wrapper_path = assert_cmd::cargo::cargo_bin("codex-session");

    let output = env
        .cmd()
        .env("CODEX_SESSION_CHILD_BIN", &wrapper_path)
        .args(["self", "version", "--format", "json"])
        .timeout(std::time::Duration::from_secs(5))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert!(
        value["child-version"].is_null(),
        "child-version must be null when child resolves to the wrapper itself; got {value}"
    );
}

#[test]
fn self_version_json_snapshot() {
    let env = TestEnv::new();
    env.make_fake_codex_printing_stdout("codex 1.2.3\n");

    let output = env
        .cmd()
        .args(["self", "version", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let mut value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    env.normalize_json(&mut value);
    assert_json_snapshot!("self_version_json", value);
}
