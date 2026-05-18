#![allow(clippy::unwrap_used)]

pub mod support;

use insta::{assert_json_snapshot, assert_snapshot};
use support::TestEnv;

#[test]
fn self_config_status_text_snapshot() {
    let env = TestEnv::new();
    env.install_base();
    env.make_fake_codex_printing_stdout("ignored");

    let output = env
        .cmd()
        .args(["self", "config-status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = env.normalize_text(&String::from_utf8(output).unwrap());
    assert_snapshot!("self_config_status_text", stdout);
}

#[test]
fn self_config_status_json_snapshot() {
    let env = TestEnv::new();
    env.install_base();
    env.make_fake_codex_printing_stdout("ignored");

    let output = env
        .cmd()
        .args(["self", "config-status", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let mut value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    env.normalize_json(&mut value);
    assert_json_snapshot!("self_config_status_json", value);
}
