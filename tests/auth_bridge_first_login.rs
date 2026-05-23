#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use support::TestEnv;

fn native_dir(env: &TestEnv) -> std::path::PathBuf {
    env.home.join(".codex")
}

fn native_auth(env: &TestEnv) -> std::path::PathBuf {
    native_dir(env).join("auth.json")
}

#[test]
fn auth_bridge_first_login() {
    let env = TestEnv::new();
    let new_payload =
        r#"{"last_refresh":"2026-06-01T00:00:00Z","tokens":{"access_token":"first-login"}}"#;

    let child_dir = env.make_fake_codex_in_dir(
        "first-login-child",
        &format!(
            r#"#!/usr/bin/env bash
cat > "$CODEX_HOME/auth.json" <<'EOF'
{new_payload}
EOF
chmod 600 "$CODEX_HOME/auth.json"
"#
        ),
    );
    let child_bin = child_dir.join("codex");

    env.cmd()
        .env("CODEX_SESSION_CHILD_BIN", &child_bin)
        .arg("exec")
        .assert()
        .success();

    assert_eq!(
        std::fs::read_to_string(env.session_dir().join("auth.json"))
            .unwrap()
            .trim_end(),
        new_payload
    );
    assert!(
        !native_auth(&env).exists(),
        "native auth should remain untouched"
    );
}
