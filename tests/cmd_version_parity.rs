#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

pub mod support;

use support::TestEnv;

#[test]
fn version_flag_and_subcommand_match_for_text() {
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
fn short_version_flag_matches_subcommand_for_text() {
    let env = TestEnv::new();
    env.make_fake_codex_printing_stdout("codex 1.2.3\n");

    let short_flag = env
        .cmd()
        .arg("-V")
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

    assert_eq!(short_flag, subcommand);
}

#[test]
fn version_flag_and_subcommand_match_for_json() {
    let env = TestEnv::new();
    env.make_fake_codex_printing_stdout("codex 1.2.3\n");

    let flag = env
        .cmd()
        .args(["--version", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let subcommand = env
        .cmd()
        .args(["version", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    assert_eq!(flag, subcommand);
}

#[test]
fn short_version_flag_matches_subcommand_for_json() {
    let env = TestEnv::new();
    env.make_fake_codex_printing_stdout("codex 1.2.3\n");

    let short_flag = env
        .cmd()
        .args(["-V", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let subcommand = env
        .cmd()
        .args(["version", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    assert_eq!(short_flag, subcommand);
}
