#![allow(clippy::unwrap_used)]

pub mod support;

use predicates::prelude::*;
use support::TestEnv;

#[test]
fn self_with_no_verb_prints_help_and_exits_zero() {
    TestEnv::new().cmd().arg("self").assert().success().stdout(
        predicate::str::starts_with("codex-session")
            .and(predicate::str::contains("self config-status")),
    );
}

#[test]
fn self_with_unknown_verb_prints_guidance_and_exits_2() {
    TestEnv::new()
        .cmd()
        .args(["self", "nope"])
        .assert()
        .code(2)
        .stdout("")
        .stderr("unknown self verb: nope\nrun: codex-session self help\n");
}

#[test]
fn self_dash_dash_help_is_unknown_verb_exit_2() {
    TestEnv::new()
        .cmd()
        .args(["self", "--help"])
        .assert()
        .code(2)
        .stdout("")
        .stderr("unknown self verb: --help\nrun: codex-session self help\n");
}

#[test]
fn self_help_dash_dash_help_is_ignored_and_prints_curated_help() {
    let expected = include_str!("../src/ui/self_help.txt");
    TestEnv::new()
        .cmd()
        .args(["self", "help", "--help"])
        .assert()
        .success()
        .stdout(expected);
}

#[test]
fn self_version_with_trailing_args_prints_version_exit_0() {
    TestEnv::new()
        .cmd()
        .args(["self", "version", "junk"])
        .assert()
        .success()
        .stdout(format!("codex-session {}\n", env!("CARGO_PKG_VERSION")));
}

#[test]
fn self_config_status_with_trailing_args_runs_normally() {
    let env = TestEnv::new();
    env.cmd()
        .args(["self", "config-status", "extra"])
        .assert()
        .success();
}

#[test]
fn self_config_merge_with_trailing_args_still_errors_without_base() {
    let env = TestEnv::new();
    let expected = format!(
        "ERROR: base config not found at {}\n",
        env.base_path().display()
    );
    env.cmd()
        .args(["self", "config-merge", "extra"])
        .assert()
        .code(1)
        .stdout("")
        .stderr(expected);
}

#[test]
fn self_show_local_with_trailing_args_returns_zero_when_target_missing() {
    TestEnv::new()
        .cmd()
        .args(["self", "show-local", "extra"])
        .assert()
        .success()
        .stdout("");
}

#[test]
fn self_empty_verb_is_treated_as_help() {
    // Bash `${1:-help}` treats an empty first token as the default verb.
    TestEnv::new()
        .cmd()
        .args(["self", ""])
        .assert()
        .success()
        .stdout(
            predicate::str::starts_with("codex-session")
                .and(predicate::str::contains("self config-status")),
        );
}

#[cfg(unix)]
#[test]
fn self_non_utf8_verb_is_unknown_verb_exit_2() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt as _;

    // 0xff is never valid UTF-8; bash would fall through to the unknown-verb
    // arm. The renderer must use lossy display rather than coercing the
    // token to "help".
    let bad = OsStr::from_bytes(b"\xff\xfe");
    TestEnv::new()
        .cmd()
        .arg("self")
        .arg(bad)
        .assert()
        .code(2)
        .stdout("");
}
