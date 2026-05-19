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
