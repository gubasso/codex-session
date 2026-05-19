#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use std::os::unix::fs::PermissionsExt as _;

use support::TestEnv;

fn native_dir(env: &TestEnv) -> std::path::PathBuf {
    env.home.join(".codex")
}

fn native_auth(env: &TestEnv) -> std::path::PathBuf {
    native_dir(env).join("auth.json")
}

#[test]
fn auth_bridge_hardlink_refused() {
    let env = TestEnv::new();
    let sentinel = env.home.join("sentinel-auth.json");
    let new_payload =
        r#"{"tokens":{"last_refresh":"2026-06-01T00:00:00Z","access_token":"newer"}}"#;

    std::fs::create_dir_all(native_dir(&env)).unwrap();
    std::fs::set_permissions(native_dir(&env), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(&sentinel, "sentinel-unchanged").unwrap();
    std::fs::set_permissions(&sentinel, std::fs::Permissions::from_mode(0o600)).unwrap();

    let child_dir = env.make_fake_codex_in_dir(
        "hardlink-child",
        &format!(
            r#"#!/usr/bin/env bash
cat > "$CODEX_HOME/auth.json" <<'EOF'
{new_payload}
EOF
chmod 600 "$CODEX_HOME/auth.json"
rm -f "$HOME/.codex/auth.json"
ln "$HOME/sentinel-auth.json" "$HOME/.codex/auth.json"
"#
        ),
    );
    let child_bin = child_dir.join("codex");

    let output = env
        .cmd()
        .args(["--log-stderr", "-v", "exec"])
        .env("CODEX_SESSION_CHILD_BIN", &child_bin)
        .assert()
        .success()
        .get_output()
        .stderr
        .clone();
    let stderr = String::from_utf8(output).unwrap();

    assert!(
        stderr.contains("hardlinked"),
        "missing hardlink warning: {stderr}"
    );
    assert_eq!(
        std::fs::read_to_string(&sentinel).unwrap(),
        "sentinel-unchanged"
    );
    assert_eq!(
        std::fs::read_to_string(native_auth(&env)).unwrap(),
        "sentinel-unchanged"
    );
}
