#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use support::{TestEnv, fixture_path};

#[test]
fn passthrough_sets_codex_home_and_profile_env_without_leaking_wrapper_namespace() {
    let env = TestEnv::new();
    env.install_profile(
        "default",
        "settings-layers:\n  - base\n",
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
    assert!(!stdout.contains("LEAKED:CODEX_SESSION_PROFILE"));
}
