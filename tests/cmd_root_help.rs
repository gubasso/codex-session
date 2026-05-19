#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

pub mod support;

use insta::assert_snapshot;
use predicates::prelude::*;
use support::{TestEnv, color};

#[test]
fn root_help_succeeds() {
    let env = TestEnv::new();
    let mut cmd = env.cmd();
    color::with_no_color(&mut cmd)
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("wrapper around `codex`"))
        .stdout(predicate::str::contains("Usage:"))
        .stderr("");
}

#[test]
fn root_help_snapshot() {
    let env = TestEnv::new();
    let mut cmd = env.cmd();
    let stdout = color::with_no_color(&mut cmd)
        .arg("--help")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let normalized = env.normalize_text(&String::from_utf8(stdout).unwrap());
    assert_snapshot!("root_help", normalized);
}

/// `--help` and `help` must serve the same curated help text.
#[test]
fn root_help_flag_and_subcommand_agree() {
    let env = TestEnv::new();
    let mut flag_cmd = env.cmd();
    let flag_out = color::with_no_color(&mut flag_cmd)
        .arg("--help")
        .output()
        .unwrap();
    let mut subcmd = env.cmd();
    let subcmd_out = color::with_no_color(&mut subcmd)
        .arg("help")
        .output()
        .unwrap();
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
    let env = TestEnv::new();
    let mut cmd = env.cmd();
    color::with_no_color(&mut cmd)
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
    let env = TestEnv::new();
    let mut cmd = env.cmd();
    color::with_no_color(&mut cmd)
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"));
}

#[test]
fn help_subcommand_prints_wrapper_help() {
    let env = TestEnv::new();
    let mut cmd = env.cmd();
    color::with_no_color(&mut cmd)
        .arg("help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"));
}
