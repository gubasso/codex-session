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
fn auth_bridge_seed_and_persist() {
    let env = TestEnv::new();
    let native_payload =
        r#"{"last_refresh":"2026-01-01T00:00:00Z","tokens":{"access_token":"old"}}"#;

    std::fs::create_dir_all(native_dir(&env)).unwrap();
    std::fs::set_permissions(native_dir(&env), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(native_auth(&env), native_payload).unwrap();
    std::fs::set_permissions(native_auth(&env), std::fs::Permissions::from_mode(0o600)).unwrap();

    let child_dir = env.make_fake_codex_in_dir("auth-child", "#!/usr/bin/env bash\nexit 0\n");
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
        native_payload
    );
    assert_eq!(
        std::fs::read_to_string(native_auth(&env))
            .unwrap()
            .trim_end(),
        native_payload
    );
    assert_eq!(
        std::fs::metadata(env.session_dir().join("auth.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(native_dir(&env))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
}

#[test]
fn second_run_does_not_overwrite_existing_session_auth() {
    let env = TestEnv::new();
    let initial_payload =
        r#"{"last_refresh":"2026-06-01T00:00:00Z","tokens":{"access_token":"initial"}}"#;
    let refreshed_payload =
        r#"{"last_refresh":"2026-06-01T00:01:00Z","tokens":{"access_token":"refreshed"}}"#;

    std::fs::create_dir_all(native_dir(&env)).unwrap();
    std::fs::set_permissions(native_dir(&env), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(native_auth(&env), initial_payload).unwrap();
    std::fs::set_permissions(native_auth(&env), std::fs::Permissions::from_mode(0o600)).unwrap();

    let child_dir = env.make_fake_codex_in_dir("one-shot-import", "#!/usr/bin/env bash\nexit 0\n");
    let child_bin = child_dir.join("codex");

    env.cmd()
        .arg("exec")
        .env("CODEX_SESSION_CHILD_BIN", &child_bin)
        .assert()
        .success();

    let session_auth = env.session_dir().join("auth.json");
    std::fs::write(native_auth(&env), refreshed_payload).unwrap();
    std::fs::set_permissions(native_auth(&env), std::fs::Permissions::from_mode(0o600)).unwrap();

    env.cmd()
        .arg("exec")
        .env("CODEX_SESSION_CHILD_BIN", &child_bin)
        .assert()
        .success();

    assert_eq!(
        std::fs::read_to_string(&session_auth).unwrap().trim_end(),
        initial_payload
    );
}
