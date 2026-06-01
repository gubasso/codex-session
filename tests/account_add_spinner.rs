#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use std::path::Path;
use std::process::{Command, ExitStatus};

use support::TestEnv;

macro_rules! skip_without_script {
    () => {
        if !Path::new("/usr/bin/script").exists() {
            eprintln!("skipping: /usr/bin/script not available on this platform");
            return;
        }
    };
}

fn fake_login_script() -> String {
    [
        r#"#!/usr/bin/env bash
case "${"#,
        r#"1:-}" in
    --version)
        printf '%s\n' 'codex 0.134.0'
        exit 0
        ;;
    login)
        printf '%s\n' 'CHILD_LOGIN_START'
        mkdir -p "$CODEX_HOME"
        cat > "$CODEX_HOME/auth.json" << 'EOF'
{"tokens":{"access_token":"fresh-at","refresh_token":"fresh-rt","account_id":"a"}}
EOF
        chmod 600 "$CODEX_HOME/auth.json"
        exit 0
        ;;
    logout)
        exit 0
        ;;
esac
exit 0
"#,
    ]
    .concat()
}

fn run_add_via_pty(env: &TestEnv, child_bin: &Path, account: &str) -> (ExitStatus, String) {
    let bin = assert_cmd::cargo::cargo_bin("codex-session");
    let transcript = env.tmp.path().join(format!("add-{account}.typescript"));
    let cmd_str = format!("{} account add {account}", bin.display());
    let status = Command::new("/usr/bin/script")
        .args(["-qec", &cmd_str, &transcript.display().to_string()])
        .env_clear()
        .env("HOME", &env.home)
        .env("XDG_CACHE_HOME", &env.cache)
        .env("XDG_CONFIG_HOME", &env.config_home)
        .env("XDG_STATE_HOME", &env.state_home)
        .env("XDG_RUNTIME_DIR", &env.runtime)
        .env("PATH", env.fake_bin.display().to_string())
        .env("CODEX_SESSION_CHILD_BIN", child_bin)
        .env("NO_COLOR", "1")
        .env("TERM", "dumb")
        .status()
        .unwrap();
    let transcript = std::fs::read_to_string(transcript).unwrap_or_default();
    (status, transcript)
}

#[test]
fn add_noninteractive_rejects_without_spinner_artifacts() {
    let env = TestEnv::new_empty();

    let output = env
        .cmd()
        .args(["account", "add", "new-acct"])
        .assert()
        .failure()
        .get_output()
        .clone();
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!stderr.contains('\u{1b}'), "stderr contains ANSI escapes");
    assert!(!stderr.contains('⠋'), "stderr contains spinner frame ⠋");
    assert!(!stderr.contains('⠙'), "stderr contains spinner frame ⠙");
}

#[test]
fn add_success_creates_account_via_pty() {
    skip_without_script!();
    let env = TestEnv::new_empty();
    let child_dir = env.make_fake_codex_in_dir("add-child", &fake_login_script());
    let child_bin = child_dir.join("codex");

    let (status, transcript) = run_add_via_pty(&env, &child_bin, "new-acct");
    assert!(
        status.success(),
        "add failed: {status}; transcript:\n{transcript}"
    );

    let seed = std::fs::read_to_string(env.named_account_auth_seed("new-acct")).unwrap();
    assert!(seed.contains("fresh-at"), "seed: {seed}");
    assert!(seed.contains("fresh-rt"), "seed: {seed}");
}

#[test]
fn add_pty_transcript_has_clean_child_login_line() {
    skip_without_script!();
    let env = TestEnv::new_empty();
    let child_dir = env.make_fake_codex_in_dir("add-child-clean", &fake_login_script());
    let child_bin = child_dir.join("codex");

    let (status, transcript) = run_add_via_pty(&env, &child_bin, "new-acct");
    assert!(
        status.success(),
        "add failed: {status}; transcript:\n{transcript}"
    );

    let child_line = transcript
        .lines()
        .find(|line| line.contains("CHILD_LOGIN_START"))
        .unwrap_or("");
    assert!(
        !child_line.is_empty(),
        "missing child login line in transcript:\n{transcript}"
    );
    assert!(
        !child_line.contains('\u{1b}'),
        "spinner ANSI leaked into child line: {child_line:?}"
    );
}
