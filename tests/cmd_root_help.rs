#![allow(clippy::unwrap_used)]

pub mod support;

use predicates::prelude::*;
use support::TestEnv;

#[test]
fn root_help_succeeds() {
    TestEnv::new()
        .cmd()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("wrapper around `codex`"))
        .stdout(predicate::str::contains("Usage:"))
        .stderr("");
}

/// `--help` and `help` must serve the same curated help text.
#[test]
fn root_help_flag_and_subcommand_agree() {
    let env = TestEnv::new();
    let flag_out = env.cmd().arg("--help").output().unwrap();
    let subcmd_out = env.cmd().arg("help").output().unwrap();
    assert!(flag_out.status.success());
    assert!(subcmd_out.status.success());
    assert_eq!(
        String::from_utf8(flag_out.stdout).unwrap(),
        String::from_utf8(subcmd_out.stdout).unwrap(),
        "`--help` and `help` must print the same wrapper help text"
    );
}

#[test]
fn root_version_succeeds() {
    TestEnv::new()
        .cmd()
        .arg("--version")
        .assert()
        .success()
        .stdout(
            predicate::str::is_match(
                "^codex-session \\d+\\.\\d+\\.\\d+\\ncodex \\(unresolved\\)\\n$",
            )
            .unwrap(),
        )
        .stderr("");
}

#[test]
fn no_arg_invocation_prints_wrapper_help() {
    TestEnv::new()
        .cmd()
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"));
}

#[test]
fn help_subcommand_prints_wrapper_help() {
    TestEnv::new()
        .cmd()
        .arg("help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"));
}

#[test]
fn double_dash_help_is_passed_through() {
    let env = TestEnv::new();
    env.make_fake_codex();
    env.cmd().args(["--", "--help"]).assert().success();
    assert_eq!(env.argc(), "1\n");
    assert_eq!(env.argv(), vec![String::from("--help")]);
}
