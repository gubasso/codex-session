#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

pub mod support;

use insta::{assert_json_snapshot, assert_snapshot};
use support::TestEnv;

#[test]
fn version_text_snapshot() {
    let env = TestEnv::new();
    env.make_fake_codex_printing_stdout("codex 1.2.3\n");

    let output = env
        .cmd()
        .arg("version")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = env.normalize_text(&String::from_utf8(output).unwrap());
    assert_snapshot!("version_text", stdout);
}

#[test]
fn version_flag_matches_subcommand_output() {
    let env = TestEnv::new();
    env.make_fake_codex_printing_stdout("codex 1.2.3\n");

    let flag = env
        .cmd()
        .arg("--version")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let subcommand = env
        .cmd()
        .arg("version")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(flag, subcommand);
}

#[test]
fn version_does_not_recurse_when_child_bin_points_at_wrapper() {
    let env = TestEnv::new();
    let wrapper_path = assert_cmd::cargo::cargo_bin("codex-session");

    let output = env
        .cmd()
        .env("CODEX_SESSION_CHILD_BIN", &wrapper_path)
        .args(["version", "--format", "json"])
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
fn version_json_snapshot() {
    let env = TestEnv::new();
    env.make_fake_codex_printing_stdout("codex 1.2.3\n");

    let output = env
        .cmd()
        .args(["version", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let mut value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    env.normalize_json(&mut value);
    assert_json_snapshot!("version_json", value);
}
