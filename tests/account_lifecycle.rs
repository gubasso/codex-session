#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use predicates::prelude::*;
use support::TestEnv;

fn write_native_auth(env: &TestEnv, body: &str) {
    use std::os::unix::fs::PermissionsExt as _;

    let native_dir = env.home.join(".codex");
    std::fs::create_dir_all(&native_dir).unwrap();
    let path = native_dir.join("auth.json");
    std::fs::write(&path, body).unwrap();
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o600);
    std::fs::set_permissions(path, perms).unwrap();
}

#[test]
fn account_add_creates_dir() {
    let env = TestEnv::new();
    env.cmd()
        .args(["account", "add", "work"])
        .assert()
        .success()
        .stdout(predicate::str::contains("account added: work"));
    assert!(env.named_account_root("work").is_dir());
    assert!(!env.state_session_root().join("accounts/.trash").exists());
}

#[test]
fn account_add_from_native_copies_auth() {
    let env = TestEnv::new();
    write_native_auth(&env, "{\"token\":\"abc\"}\n");
    env.cmd()
        .args(["account", "add", "work", "--from-native"])
        .assert()
        .success();
    assert_eq!(
        std::fs::read_to_string(env.named_account_auth_seed("work")).unwrap(),
        "{\"token\":\"abc\"}\n"
    );
}

#[test]
fn account_add_rejects_duplicate() {
    let env = TestEnv::new();
    env.cmd()
        .args(["account", "add", "work"])
        .assert()
        .success();
    env.cmd()
        .args(["account", "add", "work"])
        .assert()
        .failure()
        .code(78);
}

#[test]
fn account_remove_archives_to_trash() {
    let env = TestEnv::new();
    env.cmd()
        .args(["account", "add", "work"])
        .assert()
        .success();
    env.cmd()
        .args(["account", "remove", "work"])
        .assert()
        .success()
        .stdout(predicate::str::contains("account removed: work"));
    assert!(!env.named_account_root("work").exists());
    let trash = env.state_session_root().join("accounts/.trash");
    let entries: Vec<_> = std::fs::read_dir(trash).unwrap().collect();
    assert_eq!(entries.len(), 1);
}

#[test]
fn account_list_excludes_trash() {
    let env = TestEnv::new();
    env.cmd()
        .args(["account", "add", "work"])
        .assert()
        .success();
    env.cmd()
        .args(["account", "remove", "work"])
        .assert()
        .success();
    env.cmd()
        .args(["account", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("(no accounts)"));
}

#[test]
fn account_use_pins_lru() {
    let env = TestEnv::new();
    env.cmd()
        .args(["account", "add", "personal"])
        .assert()
        .success();
    env.cmd()
        .args(["account", "use", "personal"])
        .assert()
        .success();
    assert_eq!(
        std::fs::read_to_string(env.last_account_path()).unwrap(),
        "personal"
    );
    env.cmd()
        .args(["account", "current", "--format", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"name\": \"personal\""))
        .stdout(predicate::str::contains("\"source\": \"lru\""));
}
