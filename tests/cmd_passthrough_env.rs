#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use support::{TestEnv, fixture_path};

#[test]
fn passthrough_sets_codex_home_and_profile_env_without_leaking_wrapper_namespace() {
    let env = TestEnv::new();
    env.install_config_recipe(
        "default",
        "config-layers:\n  - base\n",
        &[("base", "[env]\nHELLO = \"world\"\n")],
    );

    let output = env
        .cmd()
        .env("CODEX_SESSION_CHILD_BIN", fixture_path("echo-env.sh"))
        .args(["exec"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = env.normalize_text(&String::from_utf8(output).unwrap());

    assert!(stdout.contains("CODEX_HOME="));
    assert!(stdout.contains("HELLO=world"));
    assert!(stdout.contains("CODEX_SESSION_REENTRY=1"));
    assert!(!stdout.contains("LEAKED:CODEX_SESSION_CONFIG_RECIPE"));
}

#[test]
fn passthrough_scrubs_codex_session_group_and_other_wrapper_env() {
    let env = TestEnv::new();

    let output = env
        .cmd()
        .env("CODEX_SESSION_CHILD_BIN", fixture_path("echo-env.sh"))
        .env("CODEX_SESSION_GROUP", "test")
        .env("CODEX_SESSION_FOO", "bar")
        .args(["exec"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(output).unwrap();

    assert!(!stdout.contains("LEAKED:CODEX_SESSION_GROUP=test"));
    assert!(!stdout.contains("LEAKED:CODEX_SESSION_FOO=bar"));
    assert!(stdout.contains("CODEX_SESSION_REENTRY=1"));
}
