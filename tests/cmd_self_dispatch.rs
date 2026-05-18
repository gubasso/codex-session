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
fn self_with_unknown_verb_returns_ex_usage() {
    TestEnv::new()
        .cmd()
        .args(["self", "nope"])
        .assert()
        .code(64)
        .stdout("")
        .stderr(predicate::str::contains(
            "error: unrecognized subcommand 'nope'",
        ))
        .stderr(predicate::str::contains("Usage: codex-session self"));
}

#[test]
fn self_dash_dash_help_prints_clap_help() {
    TestEnv::new()
        .cmd()
        .args(["self", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage: codex-session self"))
        .stderr("");
}

#[test]
fn self_help_dash_dash_help_is_ignored_and_prints_curated_help() {
    TestEnv::new()
        .cmd()
        .args(["self", "help", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Usage: codex-session self help [OPTIONS]",
        ));
}

#[test]
fn self_version_with_trailing_args_returns_ex_usage() {
    TestEnv::new()
        .cmd()
        .args(["self", "version", "junk"])
        .assert()
        .code(64)
        .stdout("")
        .stderr(predicate::str::contains("unexpected argument 'junk'"));
}

#[test]
fn self_config_status_with_trailing_args_returns_ex_usage() {
    let env = TestEnv::new();
    env.cmd()
        .args(["self", "config-status", "extra"])
        .assert()
        .code(64)
        .stdout("")
        .stderr(predicate::str::contains("unexpected argument 'extra'"));
}

#[test]
fn self_config_merge_with_trailing_args_returns_ex_usage() {
    TestEnv::new()
        .cmd()
        .args(["self", "config-merge", "extra"])
        .assert()
        .code(64)
        .stdout("")
        .stderr(predicate::str::contains("unexpected argument 'extra'"));
}

#[test]
fn self_show_local_with_trailing_args_returns_ex_usage() {
    TestEnv::new()
        .cmd()
        .args(["self", "show-local", "extra"])
        .assert()
        .code(64)
        .stdout("")
        .stderr(predicate::str::contains("unexpected argument 'extra'"));
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
fn self_non_utf8_verb_returns_ex_usage() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt as _;

    // 0xff is never valid UTF-8; clap must surface a usage error rather than
    // coercing the token to "help".
    let bad = OsStr::from_bytes(b"\xff\xfe");
    TestEnv::new()
        .cmd()
        .arg("self")
        .arg(bad)
        .assert()
        .code(64)
        .stdout("")
        .stderr(predicate::str::contains("unrecognized subcommand"));
}
