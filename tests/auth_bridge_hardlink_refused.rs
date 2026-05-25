#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use std::os::unix::fs::PermissionsExt as _;

use support::TestEnv;

/// Auth seed materialization reads the account's `auth.json` via
/// `secure_file_read` which refuses hardlinks (nlink > 1). When the
/// account auth seed is a hardlink, pass-through must fail with exit 75.
#[test]
fn auth_bridge_hardlink_refused() {
    let env = TestEnv::new();
    let sentinel = env.home.join("sentinel-auth.json");

    // Create a sentinel file and hardlink the account auth seed to it.
    std::fs::write(&sentinel, "sentinel-unchanged").unwrap();
    std::fs::set_permissions(&sentinel, std::fs::Permissions::from_mode(0o600)).unwrap();
    let auth_seed = env.named_account_auth_seed("default");
    std::fs::remove_file(&auth_seed).ok();
    std::fs::hard_link(&sentinel, &auth_seed).unwrap();

    let child_dir = env.make_fake_codex_in_dir("hardlink-child", "#!/usr/bin/env bash\nexit 0\n");
    let child_bin = child_dir.join("codex");

    let output = env
        .cmd()
        .args(["--log-stderr", "-v", "exec"])
        .env("CODEX_SESSION_CHILD_BIN", &child_bin)
        .assert()
        .code(75)
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
    assert!(!env.session_dir().join("auth.json").exists());
}
