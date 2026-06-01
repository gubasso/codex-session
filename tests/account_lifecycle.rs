#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use predicates::prelude::*;
use support::TestEnv;

fn has_ansi(bytes: &[u8]) -> bool {
    String::from_utf8_lossy(bytes).contains('\u{1b}')
}

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
    let stdout = env
        .cmd()
        .args(["account", "remove", "work", "--yes", "--format", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"verb\": \"removed\""))
        .stdout(predicate::str::contains("\"name\": \"work\""))
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(value["verb"], "removed");
    assert_eq!(value["name"], "work");
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
    std::fs::write(env.last_account_path(), "throwaway").unwrap();
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
fn account_use_subcommand_is_rejected() {
    let env = TestEnv::new();
    env.seed_account("personal", "{\"token\":\"abc\"}\n");
    env.cmd()
        .args(["account", "use", "personal"])
        .assert()
        .failure()
        .code(64);
}

#[test]
fn account_remove_text_splits_marker_color() {
    let env = TestEnv::new();
    env.seed_account("personal", "{\"token\":\"abc\"}\n");
    let output = env
        .cmd()
        .env("FORCE_COLOR", "1")
        .args(["account", "remove", "personal", "--yes"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(output).unwrap();
    assert!(text.contains("\u{1b}[32m✓\u{1b}[0m "));
    assert!(text.contains("\u{1b}[1m\u{1b}[32maccount removed:\u{1b}[0m"));
    assert!(text.contains("\u{1b}[1mpersonal\u{1b}[0m"));
    assert!(text.contains("\u{1b}[2m  path: "));
    assert!(!text.contains("\u{1b}[1m\u{1b}[32m✓"));
}

#[test]
fn account_list_no_color_and_json_work() {
    let env = TestEnv::new();
    env.seed_account("work", "{\"token\":\"abc\"}\n");
    std::fs::write(env.last_account_path(), "default").unwrap();

    let stdout = env
        .cmd()
        .env("NO_COLOR", "1")
        .args(["account", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(!has_ansi(&stdout));

    let json = env
        .cmd()
        .args(["account", "list", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&json).unwrap();
    let accounts = value["accounts"].as_array().unwrap();
    assert_eq!(accounts.len(), 2);
    assert_eq!(value["active"]["name"], "default");
}

#[test]
fn account_add_and_refresh_help_expose_format() {
    let env = TestEnv::new();
    for args in [
        ["account", "add", "--help"],
        ["account", "refresh", "--help"],
    ] {
        let stdout = env
            .cmd()
            .args(args)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let text = String::from_utf8(stdout).unwrap();
        assert!(text.contains("--format <FMT>"));
    }
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
