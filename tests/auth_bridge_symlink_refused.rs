#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use std::os::unix::fs::symlink;

use predicates::prelude::*;
use support::TestEnv;

/// Auth seed materialization reads the account's `auth.json` via
/// `secure_file_read` which opens with `O_NOFOLLOW`. When the account
/// auth seed is a symlink, pass-through must fail with exit 75.
#[test]
fn auth_bridge_symlink_refused() {
    let env = TestEnv::new();
    let sentinel = env.home.join("sentinel-auth.json");
    std::fs::write(&sentinel, "sentinel-data").unwrap();
    let auth_seed = env.named_account_auth_seed("default");
    std::fs::remove_file(&auth_seed).ok();
    symlink(&sentinel, &auth_seed).unwrap();

    let child_dir = env.make_fake_codex_in_dir("symlink-child", "#!/usr/bin/env bash\nexit 0\n");
    let child_bin = child_dir.join("codex");

    env.cmd()
        .env("CODEX_SESSION_CHILD_BIN", &child_bin)
        .arg("exec")
        .assert()
        .failure()
        .code(75)
        .stderr(predicate::str::contains("refused symlink"))
        .stderr(predicate::str::contains("sentinel-data").not());

    assert_eq!(std::fs::read_to_string(&sentinel).unwrap(), "sentinel-data");
    assert!(!env.session_dir().join("auth.json").exists());
}
