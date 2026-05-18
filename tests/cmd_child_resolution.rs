#![allow(clippy::unwrap_used)]

pub mod support;

use support::TestEnv;

#[test]
fn child_bin_override_wins_over_path() {
    let env = TestEnv::new();
    env.make_fake_codex_printing_stdout("PATH");
    let override_dir =
        env.make_fake_codex_in_dir("override-bin", "#!/usr/bin/env bash\nprintf 'OVERRIDE'");
    let override_bin = override_dir.join("codex");

    env.cmd()
        .env("CODEX_SESSION_CHILD_BIN", &override_bin)
        .arg("exec")
        .assert()
        .success()
        .stdout("OVERRIDE");
}

#[test]
fn child_bin_override_not_executable_exits_126() {
    let env = TestEnv::new();
    let override_dir = env.make_non_executable_codex_in_dir("override-non-exec", "not executable");
    let override_bin = override_dir.join("codex");

    env.cmd()
        .env("CODEX_SESSION_CHILD_BIN", &override_bin)
        .arg("exec")
        .assert()
        .code(126)
        .stdout("")
        .stderr(predicates::str::contains(
            "codex-session: failed to execute wrapped codex binary",
        ));
}

#[test]
fn child_bin_override_pointing_at_directory_exits_126() {
    let env = TestEnv::new();
    // A directory inherits the default 0o755 perms which include execute
    // bits. Make sure we still reject it as "not executable" rather than
    // letting the downstream `exec()` failure surface as exit 74.
    let dir = env.tmp.path().join("not-a-file");
    std::fs::create_dir_all(&dir).unwrap();

    env.cmd()
        .env("CODEX_SESSION_CHILD_BIN", &dir)
        .arg("exec")
        .assert()
        .code(126)
        .stdout("")
        .stderr(predicates::str::contains(
            "codex-session: failed to execute wrapped codex binary",
        ));
}

#[test]
fn missing_child_bin_override_exits_127() {
    let env = TestEnv::new();
    let missing = env.tmp.path().join("missing-bin/codex");

    env.cmd()
        .env("CODEX_SESSION_CHILD_BIN", &missing)
        .arg("exec")
        .assert()
        .code(127)
        .stdout("")
        .stderr(predicates::str::contains(
            "hint:  set CODEX_SESSION_CHILD_BIN or add codex to PATH and retry",
        ));
}
