#![allow(clippy::unwrap_used)]

pub mod support;

use insta::assert_json_snapshot;
use predicates::prelude::*;
use support::TestEnv;

#[test]
fn config_show_local_prints_only_local_project_sections() {
    let env = TestEnv::new();
    env.install_base();
    env.install_target_with_local();
    let expected = "[projects.\"/tmp/example\"]\ntrust_level = \"trusted\"\n\n\
                    [projects.\"/tmp/other\"]\ntrust_level = \"untrusted\"\n";
    env.cmd()
        .args(["config", "show-local"])
        .assert()
        .success()
        .stdout(expected);
}

#[test]
fn show_local_with_target_and_no_base_prints_all_bracketed_sections() {
    let env = TestEnv::new();
    env.install_target_with_local();
    env.cmd()
        .args(["config", "show-local"])
        .assert()
        .success()
        .stdout(predicate::str::contains("[projects.\"/tmp/example\"]"));
}

#[test]
fn show_local_with_neither_base_nor_target_prints_nothing() {
    TestEnv::new()
        .cmd()
        .args(["config", "show-local"])
        .assert()
        .success()
        .stdout("");
}

#[test]
fn config_show_local_json_snapshot() {
    let env = TestEnv::new();
    env.install_base();
    env.install_target_with_local();

    let output = env
        .cmd()
        .args(["config", "show-local", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let mut value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    env.normalize_json(&mut value);
    assert_json_snapshot!("config_show_local_json", value);
}
