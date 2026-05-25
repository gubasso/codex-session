#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use predicates::prelude::*;
use support::TestEnv;

#[test]
fn account_add_creates_dir() {
    let env = TestEnv::new();
    env.seed_account("work", "{\"token\":\"abc\"}\n");
    assert!(env.named_account_root("work").is_dir());
    assert!(!env.state_session_root().join("accounts/.trash").exists());
}

#[test]
fn seed_account_creates_expected_structure() {
    let env = TestEnv::new();
    env.seed_account("work", "{\"token\":\"abc\"}\n");
    assert!(env.named_account_root("work").is_dir());
    assert!(env.named_groups_root("work").is_dir());
    assert_eq!(
        std::fs::read_to_string(env.named_account_auth_seed("work")).unwrap(),
        "{\"token\":\"abc\"}\n"
    );
    assert_eq!(
        std::fs::read_to_string(env.last_account_path()).unwrap(),
        "work"
    );
}

#[test]
fn account_add_non_interactive_fails() {
    let env = TestEnv::new();
    env.cmd()
        .args(["account", "add", "work"])
        .assert()
        .failure()
        .code(64);
}

#[test]
fn account_remove_deletes_permanently() {
    let env = TestEnv::new();
    env.seed_account("work", "{\"token\":\"abc\"}\n");
    env.cmd()
        .args(["account", "remove", "work", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("account removed: work"));
    assert!(!env.named_account_root("work").exists());
}

#[test]
fn account_list_excludes_trash() {
    let env = TestEnv::new_empty();
    env.seed_account("work", "{\"token\":\"abc\"}\n");
    env.cmd()
        .args(["account", "remove", "work", "--yes"])
        .assert()
        .success();
    env.cmd()
        .args(["account", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("(no accounts)"));
}

#[test]
fn account_remove_warns_on_active_account() {
    let env = TestEnv::new();
    env.seed_account("throwaway", "{\"token\":\"abc\"}\n");
    env.cmd()
        .args(["account", "use", "throwaway"])
        .assert()
        .success();
    env.cmd()
        .args(["account", "remove", "throwaway", "--yes"])
        .assert()
        .success()
        .stderr(predicate::str::contains("current active account"));
}

#[test]
fn account_remove_warns_on_recent_sessions() {
    let env = TestEnv::new();
    env.seed_account("recent", "{\"token\":\"abc\"}\n");
    let groups = env.named_groups_root("recent");
    std::fs::create_dir_all(groups.join("test-group")).unwrap();
    env.cmd()
        .args(["account", "remove", "recent", "--yes"])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "session group(s) used in the last 24h",
        ));
}

#[test]
fn account_use_pins_lru() {
    let env = TestEnv::new();
    env.seed_account("personal", "{\"token\":\"abc\"}\n");
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

#[test]
fn account_remove_non_interactive_requires_yes() {
    let env = TestEnv::new();
    env.seed_account("work", "{\"token\":\"abc\"}\n");
    env.cmd()
        .args(["account", "remove", "work"])
        .assert()
        .failure()
        .code(64);
}
