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
        .stdout(predicate::str::contains("Wrapper around `codex`"))
        .stdout(predicate::str::contains("Usage:"))
        .stdout(predicate::str::contains("Commands:"))
        .stdout(predicate::str::contains("version"))
        .stdout(predicate::str::contains("completion"))
        .stdout(predicate::str::contains("config"))
        .stdout(predicate::str::contains("help"))
        .stdout(predicate::str::contains("CODEX_SESSION_CHILD_BIN"))
        .stdout(predicate::str::contains("Use `--` to force pass-through"))
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

/// `--help` and `help` must serve the same clap-rendered help text.
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
fn no_arg_invocation_matches_long_help() {
    let env = TestEnv::new();
    let mut bare_cmd = env.cmd();
    let bare_out = color::with_no_color(&mut bare_cmd).output().unwrap();
    let mut help_cmd = env.cmd();
    let help_out = color::with_no_color(&mut help_cmd)
        .arg("--help")
        .output()
        .unwrap();
    assert!(bare_out.status.success());
    assert!(help_out.status.success());
    assert_eq!(
        String::from_utf8(bare_out.stdout).unwrap(),
        String::from_utf8(help_out.stdout).unwrap(),
        "bare invocation must print the same long help as `--help`"
    );
}

#[test]
fn root_short_help_is_shorter() {
    let env = TestEnv::new();
    let mut short_cmd = env.cmd();
    let short_out = color::with_no_color(&mut short_cmd)
        .arg("-h")
        .output()
        .unwrap();
    let mut long_cmd = env.cmd();
    let long_out = color::with_no_color(&mut long_cmd)
        .arg("--help")
        .output()
        .unwrap();
    assert!(short_out.status.success());
    assert!(long_out.status.success());

    let short_stdout = String::from_utf8(short_out.stdout).unwrap();
    let long_stdout = String::from_utf8(long_out.stdout).unwrap();

    for verb in ["version", "completion", "config", "help"] {
        assert!(
            short_stdout.contains(verb),
            "short help must include verb `{verb}`"
        );
    }
    assert!(
        short_stdout.len() < long_stdout.len(),
        "short help should be shorter than long help"
    );
    assert!(!short_stdout.contains("CODEX_SESSION_CHILD_BIN"));
}
