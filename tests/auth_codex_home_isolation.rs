//! Tests that auth operations (login/logout) use an isolated `CODEX_HOME`
//! so that one account's login/logout cycle never touches another account's
//! tokens.
#![allow(clippy::unwrap_used, clippy::panic)]
#![allow(missing_docs)]

mod support;

use std::path::Path;
use std::process::Command;

use support::TestEnv;

macro_rules! skip_without_script {
    () => {
        if !Path::new("/usr/bin/script").exists() {
            eprintln!("skipping: /usr/bin/script not available on this platform");
            return;
        }
    };
}

fn fake_login_script(extra_before_login: &str) -> String {
    format!(
        "#!/usr/bin/env bash\n\
        case \"$1\" in\n\
        login)\n\
        {extra_before_login}\
        mkdir -p \"$CODEX_HOME\"\n\
        printf '%s' \
        '{{\"tokens\":{{\"access_token\":\"fresh-at\",\
        \"refresh_token\":\"fresh-rt\",\"account_id\":\"a\"}}}}' \
        > \"$CODEX_HOME/auth.json\"\n\
        chmod 600 \"$CODEX_HOME/auth.json\"\n\
        ;;\n\
        logout) ;;\n\
        esac\n"
    )
}

fn run_refresh_via_pty(env: &TestEnv, child_bin: &Path, account: &str) -> std::process::ExitStatus {
    let bin = assert_cmd::cargo::cargo_bin("codex-session");
    let cmd_str = format!("{} account refresh {account}", bin.display());
    Command::new("/usr/bin/script")
        .args(["-qc", &cmd_str, "/dev/null"])
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
        .unwrap()
}

/// The fake codex child receives `CODEX_HOME` under `auth-ops/`.
#[test]
fn refresh_sets_isolated_codex_home_in_child() {
    skip_without_script!();
    let env = TestEnv::new();
    env.seed_account("target", r#"{"tokens":{"access_token":"old"}}"#);

    let log = env.tmp.path().join("codex-home.log");
    let extra = format!("printf '%s' \"$CODEX_HOME\" >> '{}'\n", log.display());
    let child_dir = env.make_fake_codex_in_dir("probe", &fake_login_script(&extra));
    let child_bin = child_dir.join("codex");

    let st = run_refresh_via_pty(&env, &child_bin, "target");
    assert!(st.success(), "refresh failed: {st}");

    let home =
        std::fs::read_to_string(&log).unwrap_or_else(|_| panic!("missing {}", log.display()));
    assert!(!home.is_empty(), "child got empty CODEX_HOME");
    assert!(
        home.contains("auth-ops"),
        "CODEX_HOME should be under auth-ops/: {home}"
    );
    assert!(
        !home.contains(".codex"),
        "CODEX_HOME must not be ~/.codex: {home}"
    );
}

/// Seed file contains the token from the isolated login.
#[test]
fn refresh_persists_auth_from_isolated_dir_to_seed() {
    skip_without_script!();
    let env = TestEnv::new();
    env.seed_account(
        "acct",
        r#"{"tokens":{"access_token":"stale","account_id":"a"}}"#,
    );

    let child_dir = env.make_fake_codex_in_dir("persist", &fake_login_script(""));
    let child_bin = child_dir.join("codex");

    let st = run_refresh_via_pty(&env, &child_bin, "acct");
    assert!(st.success(), "refresh failed: {st}");

    let seed = std::fs::read_to_string(env.named_account_auth_seed("acct")).unwrap();
    assert!(seed.contains("fresh-at"), "seed: {seed}");
    assert!(seed.contains("fresh-rt"), "seed: {seed}");
}

/// Refreshing one account does NOT modify another account's seed.
#[test]
fn refresh_one_account_preserves_other_seeds() {
    skip_without_script!();
    let env = TestEnv::new();

    let auth_b = r#"{"tokens":{"access_token":"at-b","account_id":"org"}}"#;
    env.seed_account(
        "acct-a",
        r#"{"tokens":{"access_token":"at-a","account_id":"org"}}"#,
    );
    env.seed_account("acct-b", auth_b);

    let child_dir = env.make_fake_codex_in_dir("iso", &fake_login_script(""));
    let child_bin = child_dir.join("codex");

    let st = run_refresh_via_pty(&env, &child_bin, "acct-a");
    assert!(st.success(), "refresh failed: {st}");

    let seed_a = std::fs::read_to_string(env.named_account_auth_seed("acct-a")).unwrap();
    assert!(seed_a.contains("fresh-at"), "acct-a: {seed_a}");

    let seed_b = std::fs::read_to_string(env.named_account_auth_seed("acct-b")).unwrap();
    assert_eq!(seed_b.trim(), auth_b.trim(), "acct-b changed");
}

/// `~/.codex/auth.json` is never modified by refresh.
#[test]
fn refresh_does_not_touch_native_auth() {
    skip_without_script!();
    let env = TestEnv::new();
    env.seed_account("acct", r#"{"tokens":{"access_token":"old"}}"#);
    let original = "{\"token\":\"original\"}\n";
    env.write_native_auth(original);

    let child_dir = env.make_fake_codex_in_dir("native", &fake_login_script(""));
    let child_bin = child_dir.join("codex");

    let st = run_refresh_via_pty(&env, &child_bin, "acct");
    assert!(st.success(), "refresh failed: {st}");

    let native = std::fs::read_to_string(env.home.join(".codex/auth.json")).unwrap();
    assert_eq!(native, original, "native auth was modified");
}

/// Non-interactive `account add` rejects cleanly.
#[test]
fn account_add_non_interactive_still_fails_cleanly() {
    let env = TestEnv::new();
    env.cmd()
        .args(["account", "add", "new-acct"])
        .assert()
        .failure()
        .code(64);
}
