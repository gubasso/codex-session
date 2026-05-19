#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use support::TestEnv;

#[test]
fn dry_run_prints_invocation_and_skips_exec() {
    let env = TestEnv::new();
    env.make_fake_codex();
    let assert = env
        .cmd()
        .args(["--dry-run", "exec", "foo", "bar"])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();

    assert!(
        stdout.starts_with("binary: "),
        "missing binary line: {stdout}"
    );
    assert!(stdout.contains("argv:"), "missing argv section: {stdout}");
    assert!(stdout.contains("[0] exec"), "missing argv[0]: {stdout}");
    assert!(stdout.contains("[1] foo"));
    assert!(stdout.contains("[2] bar"));
    assert!(stdout.contains("env.inherit: true"));
    assert!(stdout.contains("CODEX_SESSION_CHILD_BIN"));
    assert!(stdout.contains("CODEX_SESSION_LOG_FILE"));
    assert!(stdout.contains("CODEX_SESSION_LOG_DIR"));
    assert!(stdout.contains("CODEX_SESSION_REENTRY"));
    assert!(stdout.contains("env.set:") && stdout.contains("CODEX_SESSION_REENTRY=1"));

    assert!(!env.argc_file.exists(), "child was unexpectedly exec'd");
    assert!(!env.argv_file.exists(), "child was unexpectedly exec'd");
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

/// `--dry-run` is a wrapper-owned global flag, but the root `Cli` sets
/// `allow_external_subcommands = true`. clap's external-subcommand
/// parser greedily captures any token after the verb (including
/// `--`-prefixed ones), so `codex-session exec --dry-run foo` must
/// forward `--dry-run` to the child unchanged rather than triggering
/// the wrapper's dry-run report. This regression test locks in the
/// "pre-verb wrapper flag, post-verb child arg" distinction explicitly
/// called out in the reviewed plan.
#[test]
fn dry_run_after_verb_is_forwarded_to_child() {
    let env = TestEnv::new();
    env.make_fake_codex();
    env.cmd()
        .args(["exec", "--dry-run", "foo"])
        .assert()
        .success();

    // The fake codex would have written its argv on real exec. If the
    // wrapper had consumed `--dry-run` here, exec would have been
    // skipped (no argv file). And if it had been forwarded, the child
    // sees `exec`, `--dry-run`, `foo`.
    assert!(
        env.argv_file.exists(),
        "child was not exec'd — wrapper incorrectly consumed --dry-run after verb"
    );
    let argv = env.argv();
    assert_eq!(
        argv,
        vec![
            "exec".to_string(),
            "--dry-run".to_string(),
            "foo".to_string()
        ],
        "child argv mismatch: {argv:?}"
    );
}
