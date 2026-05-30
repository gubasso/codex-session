#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use support::TestEnv;

#[test]
fn dry_run_prints_invocation_session_and_codex_home_without_exec() {
    let env = TestEnv::new();
    env.make_fake_codex();
    env.install_config_recipe(
        "default",
        "config-layers:\n  - base\n",
        &[("base", "[env]\nHELLO = \"world\"\n")],
    );
    let assert = env
        .cmd()
        .args(["--dry-run", "exec", "foo", "bar"])
        .assert()
        .success();
    let stdout =
        env.normalize_text(&String::from_utf8(assert.get_output().stdout.clone()).unwrap());

    assert!(
        stdout.starts_with("account: default\naccount-source: auto\nbinary: "),
        "missing account/binary header lines: {stdout}"
    );
    assert!(stdout.contains("argv:"));
    assert!(stdout.contains("[0] exec"));
    assert!(stdout.contains("[1] foo"));
    assert!(stdout.contains("[2] bar"));
    assert!(stdout.contains("env.inherit: true"));
    assert!(stdout.contains("CODEX_SESSION_CHILD_BIN"));
    assert!(stdout.contains("CODEX_SESSION_LOG_FILE"));
    assert!(stdout.contains("CODEX_SESSION_LOG_DIR"));
    assert!(stdout.contains("CODEX_SESSION_REENTRY"));
    assert!(stdout.contains("CODEX_HOME="));
    assert!(stdout.contains("HELLO=world"));
    assert!(stdout.contains("CODEX_SESSION_REENTRY=1"));

    assert!(!env.argc_file.exists(), "child was unexpectedly exec'd");
    assert!(!env.argv_file.exists(), "child was unexpectedly exec'd");
}

#[test]
fn dry_run_with_account_flag_shows_flag_context() {
    let env = TestEnv::new();
    env.make_fake_codex();
    env.seed_account("work", "{\"token\":\"test\"}\n");
    let assert = env
        .cmd()
        .args(["--account", "work", "--dry-run", "exec", "hi"])
        .assert()
        .success();
    let stdout =
        env.normalize_text(&String::from_utf8(assert.get_output().stdout.clone()).unwrap());
    assert!(stdout.contains("account: work"));
    assert!(stdout.contains("account-source: flag"));
}

#[test]
fn dry_run_works_without_resolvable_child() {
    let env = TestEnv::new();
    env.cmd()
        .args(["--dry-run", "anything"])
        .assert()
        .failure()
        .code(127);
}

#[test]
fn dry_run_after_verb_is_forwarded_to_child() {
    let env = TestEnv::new();
    env.make_fake_codex();
    env.cmd()
        .args(["exec", "--dry-run", "foo"])
        .assert()
        .success();
    assert!(env.argv_file.exists());
    assert_eq!(
        env.argv(),
        vec![
            "exec".to_string(),
            "--dry-run".to_string(),
            "foo".to_string()
        ]
    );
}
