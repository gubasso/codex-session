#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use std::os::unix::fs::{PermissionsExt as _, symlink};

use predicates::prelude::*;
use support::TestEnv;

fn native_dir(env: &TestEnv) -> std::path::PathBuf {
    env.home.join(".codex")
}

fn native_auth(env: &TestEnv) -> std::path::PathBuf {
    native_dir(env).join("auth.json")
}

#[test]
fn auth_bridge_symlink_refused() {
    let env = TestEnv::new();
    let sentinel = env.home.join("sentinel-auth.json");
    std::fs::create_dir_all(native_dir(&env)).unwrap();
    std::fs::set_permissions(native_dir(&env), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(&sentinel, "sentinel-data").unwrap();
    symlink(&sentinel, native_auth(&env)).unwrap();

    let child_dir = env.make_fake_codex_in_dir("symlink-child", "#!/usr/bin/env bash\nexit 0\n");
    let child_bin = child_dir.join("codex");

    env.cmd()
        .env("CODEX_SESSION_CHILD_BIN", &child_bin)
        .arg("exec")
        .assert()
        .failure()
        .code(75)
        .stderr(predicate::str::contains("refused symlinked auth.json"))
        .stderr(predicate::str::contains(
            native_auth(&env).display().to_string(),
        ))
        .stderr(predicate::str::contains("sentinel-data").not());

    assert_eq!(std::fs::read_to_string(&sentinel).unwrap(), "sentinel-data");
    assert!(!env.session_dir().join("auth.json").exists());
}
