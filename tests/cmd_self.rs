#![allow(clippy::unwrap_used)]

pub mod support;

use predicates::prelude::*;
use support::TestEnv;

#[test]
fn self_help_exits_zero_and_prints_usage() {
    TestEnv::new()
        .cmd()
        .args(["self", "help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"));
}

#[test]
fn self_version_exits_zero_and_prints_codex_session_0_1_0() {
    TestEnv::new()
        .cmd()
        .args(["self", "version"])
        .assert()
        .success()
        .stdout("codex-session 0.1.0\n");
}

#[test]
fn self_config_status_prints_booleans_and_needs_merge() {
    let env = TestEnv::new();
    env.install_base();
    let expected = format!(
        "base:        {} (exists=true)\n\
target:      {} (exists=false)\n\
stamp:       {} (exists=false)\n\
needs_merge: yes\n",
        env.base_path().display(),
        env.target_path().display(),
        env.stamp_path().display(),
    );
    env.cmd()
        .args(["self", "config-status"])
        .assert()
        .success()
        .stdout(expected);
}

#[test]
fn self_config_status_reports_needs_merge_no_when_base_is_missing() {
    let env = TestEnv::new();
    let expected = format!(
        "base:        {} (exists=false)\n\
target:      {} (exists=false)\n\
stamp:       {} (exists=false)\n\
needs_merge: no\n",
        env.base_path().display(),
        env.target_path().display(),
        env.stamp_path().display(),
    );
    env.cmd()
        .args(["self", "config-status"])
        .assert()
        .success()
        .stdout(expected);
}

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
fn self_config_merge_forces_rewrite_when_stamp_is_fresh() {
    let env = TestEnv::new();
    env.install_base();
    env.install_target_with_local();
    std::fs::create_dir_all(env.stamp_path().parent().unwrap()).unwrap();
    std::fs::write(env.stamp_path(), "").unwrap();
    env.touch_older(&env.base_path());
    env.touch_newer(&env.stamp_path());

    let expected_stdout = format!(
        "merged: {} -> {}\n",
        env.base_path().display(),
        env.target_path().display()
    );
    env.cmd()
        .args(["self", "config-merge"])
        .assert()
        .success()
        .stdout(expected_stdout);

    let got = std::fs::read_to_string(env.target_path()).unwrap();
    let want = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/expected-merged.toml"),
    )
    .unwrap();
    assert_eq!(got, want);
}

#[test]
fn self_config_merge_without_base_errors_exactly() {
    let env = TestEnv::new();
    let expected = format!(
        "ERROR: base config not found at {}\n",
        env.base_path().display()
    );
    env.cmd()
        .args(["self", "config-merge"])
        .assert()
        .code(1)
        .stdout("")
        .stderr(expected);
}

#[test]
fn unknown_self_verb_exits_two_and_prints_guidance() {
    TestEnv::new()
        .cmd()
        .args(["self", "nope"])
        .assert()
        .code(2)
        .stdout("")
        .stderr("unknown self verb: nope\nrun: codex-session self help\n");
}
