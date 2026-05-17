#![allow(clippy::unwrap_used)]

pub mod support;

use predicates::prelude::*;
use support::TestEnv;

#[test]
fn self_show_local_prints_only_local_project_sections() {
    let env = TestEnv::new();
    env.install_base();
    env.install_target_with_local();
    let expected = "[projects.\"/tmp/example\"]\ntrust_level = \"trusted\"\n\n\
                    [projects.\"/tmp/other\"]\ntrust_level = \"untrusted\"\n";
    env.cmd()
        .args(["self", "show-local"])
        .assert()
        .success()
        .stdout(expected);
}

#[test]
fn show_local_with_target_and_no_base_prints_all_bracketed_sections() {
    let env = TestEnv::new();
    env.install_target_with_local();
    env.cmd()
        .args(["self", "show-local"])
        .assert()
        .success()
        .stdout(predicate::str::contains("[projects.\"/tmp/example\"]"));
}

#[test]
fn show_local_with_neither_base_nor_target_prints_nothing() {
    TestEnv::new()
        .cmd()
        .args(["self", "show-local"])
        .assert()
        .success()
        .stdout("");
}
