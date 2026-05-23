#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use predicates::prelude::*;
use support::TestEnv;

fn write_cooldown(env: &TestEnv, account: &str) {
    let path = env.named_account_root(account).join("cooldown.json");
    std::fs::write(
        &path,
        r#"{
    "reset_at_unix": 4102444800,
    "reason": "429 detected: \"HTTP 429 Too Many Requests\"",
    "last_429_at_unix": 4102444500,
    "snippet_truncated": "HTTP 429 Too Many Requests"
}"#,
    )
    .unwrap();
}

#[test]
fn cooldown_show_alias_and_json_work() {
    let env = TestEnv::new();
    env.cmd()
        .args(["account", "add", "work"])
        .assert()
        .success();
    env.cmd()
        .args(["account", "add", "personal"])
        .assert()
        .success();
    write_cooldown(&env, "work");

    let alias = env
        .cmd()
        .args(["account", "cooldown"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let show = env
        .cmd()
        .args(["account", "cooldown", "show"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(alias, show);

    env.cmd()
        .args(["account", "cooldown", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ACCOUNT"))
        .stdout(predicate::str::contains("work"))
        .stdout(predicate::str::contains("personal"))
        .stdout(predicate::str::contains("cooled-down"))
        .stdout(predicate::str::contains("eligible"));

    let json = env
        .cmd()
        .args(["account", "cooldown", "show", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&json).unwrap();
    let items = value.as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["account"], "personal");
    assert_eq!(items[0]["cooled-down"], false);
    assert_eq!(items[1]["account"], "work");
    assert_eq!(items[1]["cooled-down"], true);
}

#[test]
fn cooldown_show_account_filter_and_clear_modes_work() {
    let env = TestEnv::new();
    env.cmd()
        .args(["account", "add", "work"])
        .assert()
        .success();
    env.cmd()
        .args(["account", "add", "personal"])
        .assert()
        .success();
    write_cooldown(&env, "work");
    write_cooldown(&env, "personal");

    env.cmd()
        .args(["account", "cooldown", "show", "--account", "work"])
        .assert()
        .success()
        .stdout(predicate::str::contains("work"))
        .stdout(predicate::str::contains("cooled-down"))
        .stdout(predicate::str::contains("HTTP 429"));

    env.cmd()
        .args(["account", "cooldown", "clear", "--account", "work"])
        .assert()
        .success();
    assert!(
        !env.named_account_root("work")
            .join("cooldown.json")
            .exists()
    );
    assert!(
        env.named_account_root("personal")
            .join("cooldown.json")
            .exists()
    );

    env.cmd()
        .args(["account", "cooldown", "clear", "--all"])
        .assert()
        .success();
    assert!(
        !env.named_account_root("personal")
            .join("cooldown.json")
            .exists()
    );
}

#[test]
fn cooldown_clear_requires_target() {
    let env = TestEnv::new();
    env.cmd()
        .args(["account", "cooldown", "clear"])
        .assert()
        .failure()
        .code(64)
        .stderr(predicate::str::contains(
            "either --account <NAME> or --all is required",
        ));
}

#[test]
fn cooldown_clear_rejects_account_and_all_together() {
    let env = TestEnv::new();
    env.cmd()
        .args(["--account", "work", "account", "cooldown", "clear", "--all"])
        .assert()
        .failure()
        .code(64)
        .stderr(predicate::str::contains("--all conflicts with --account"));
}
