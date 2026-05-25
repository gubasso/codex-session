#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use std::os::unix::fs::PermissionsExt as _;

use support::TestEnv;

/// Auth seeding now happens via `materialize_account_auth_seed`: the
/// account's `auth.json` seed (under `accounts/<name>/auth.json`) is
/// copied into the session group dir on first run. The old
/// `import_if_missing` path (from `~/.codex/auth.json`) is deleted.
#[test]
fn auth_bridge_seed_and_persist() {
    let env = TestEnv::new();
    let seed_payload = r#"{"last_refresh":"2026-01-01T00:00:00Z","tokens":{"access_token":"old"}}"#;

    // Overwrite the default account's auth seed with a specific payload.
    env.write_account_auth_seed("default", seed_payload);

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
        seed_payload
    );
    // The account seed must remain untouched.
    assert_eq!(
        std::fs::read_to_string(env.named_account_auth_seed("default"))
            .unwrap()
            .trim_end(),
        seed_payload
    );
    assert_eq!(
        std::fs::metadata(env.session_dir().join("auth.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn second_run_does_not_overwrite_existing_session_auth() {
    let env = TestEnv::new();
    let initial_payload =
        r#"{"last_refresh":"2026-06-01T00:00:00Z","tokens":{"access_token":"initial"}}"#;
    let refreshed_payload =
        r#"{"last_refresh":"2026-06-01T00:01:00Z","tokens":{"access_token":"refreshed"}}"#;

    // Write initial auth seed.
    env.write_account_auth_seed("default", initial_payload);

    let child_dir = env.make_fake_codex_in_dir("one-shot-import", "#!/usr/bin/env bash\nexit 0\n");
    let child_bin = child_dir.join("codex");

    env.cmd()
        .arg("exec")
        .env("CODEX_SESSION_CHILD_BIN", &child_bin)
        .assert()
        .success();

    let session_auth = env.session_dir().join("auth.json");
    // Update the account seed *after* the first run.
    env.write_account_auth_seed("default", refreshed_payload);

    env.cmd()
        .arg("exec")
        .env("CODEX_SESSION_CHILD_BIN", &child_bin)
        .assert()
        .success();

    // The session auth must still contain the initial payload — the
    // materializer must not overwrite an existing session auth.
    assert_eq!(
        std::fs::read_to_string(&session_auth).unwrap().trim_end(),
        initial_payload
    );
}
