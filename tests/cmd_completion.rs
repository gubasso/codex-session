#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

pub mod support;

use support::TestEnv;

#[test]
fn bash_completion_smokes() {
    let env = TestEnv::new();
    let out = env.cmd().args(["completion", "bash"]).assert().success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    assert!(
        stdout.contains("_codex-session"),
        "missing bash marker:\n{stdout}"
    );
}

#[test]
fn zsh_completion_smokes() {
    let env = TestEnv::new();
    let out = env.cmd().args(["completion", "zsh"]).assert().success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    assert!(stdout.contains("#compdef"), "missing zsh marker:\n{stdout}");
}

#[test]
fn fish_completion_smokes() {
    let env = TestEnv::new();
    env.cmd().args(["completion", "fish"]).assert().success();
}

#[test]
fn invalid_shell_exits_64() {
    let env = TestEnv::new();
    env.cmd()
        .args(["completion", "not-a-shell"])
        .assert()
        .code(64);
}

/// `completion` must print with no writable `HOME` and no XDG variables,
/// as in a Nix build sandbox, so a package can generate the script in its
/// install phase. Before the fix, start-up created the log directory under
/// `HOME` and failed there.
#[test]
fn bash_completion_runs_with_read_only_home() {
    use std::os::unix::fs::PermissionsExt as _;

    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("read-only-home");
    std::fs::create_dir(&home).unwrap();
    std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o555)).unwrap();

    let out = assert_cmd::Command::cargo_bin("codex-session")
        .unwrap()
        .env_clear()
        .env("HOME", &home)
        .args(["completion", "bash"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    assert!(
        stdout.contains("_codex-session"),
        "missing bash marker:\n{stdout}"
    );
    assert_eq!(
        std::fs::read_dir(&home).unwrap().count(),
        0,
        "completion must write nothing under HOME"
    );
}
