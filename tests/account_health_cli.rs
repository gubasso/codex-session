#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use std::os::unix::fs::PermissionsExt as _;

use predicates::prelude::*;
use support::{FakeCodexBehavior, TEST_AUTH, TestEnv};

#[test]
fn health_rejects_account_auto() {
    let env = TestEnv::new();
    env.cmd()
        .args(["--account", "auto", "account", "health"])
        .assert()
        .failure()
        .code(64)
        .stderr(predicate::str::contains(
            "--account auto is not supported for account health",
        ));
}

#[test]
fn health_requires_ping_profile_when_not_fast() {
    let env = TestEnv::new_empty();
    env.seed_account("work", TEST_AUTH);
    std::fs::write(
        env.wrapper_user_config_path(),
        "[config-recipe]\ndefault = \"test\"\n",
    )
    .unwrap();
    env.install_config_recipe(
        "test",
        "config-layers:\n  - base\n",
        &[("base", "model = \"gpt-4.1-nano\"\n")],
    );

    env.cmd()
        .args(["account", "health"])
        .assert()
        .failure()
        .code(78)
        .stderr(predicate::str::contains("profiles/ping.config.toml"));
}

#[test]
fn login_probe_writes_split_ping_profile_file() {
    let env = TestEnv::new();
    std::fs::write(
        env.wrapper_user_config_path(),
        "[config-recipe]\ndefault = \"default\"\n",
    )
    .unwrap();
    env.install_config_recipe(
        "default",
        "config-layers:\n  - base\nprofile-files:\n  - ping\n",
        &[("base", "model = \"gpt-5.4\"\nweb_search = \"live\"\n")],
    );
    let ping_body = "model = \"gpt-5.4-mini\"\nmodel_reasoning_effort = \"minimal\"\n";
    env.write_profile_file("ping", ping_body);

    let home_capture = env.tmp.path().join("probe-home.txt");
    let profile_capture = env.tmp.path().join("probe-profile.toml");
    let args_capture = env.tmp.path().join("probe-args.txt");
    let codex = env.fake_bin.join("codex");
    let script = format!(
        r#"#!/usr/bin/env bash
set -eu
printf '%s\n' "$@" > "{args_capture}"
printf '%s\n' "$CODEX_HOME" > "{home_capture}"
[ -f "$CODEX_HOME/auth.json" ]
[ -f "$CODEX_HOME/config.toml" ]
[ ! -s "$CODEX_HOME/config.toml" ]
[ -f "$CODEX_HOME/ping.config.toml" ]
cat "$CODEX_HOME/ping.config.toml" > "{profile_capture}"
exit 0
"#,
        args_capture = args_capture.display(),
        home_capture = home_capture.display(),
        profile_capture = profile_capture.display(),
    );
    std::fs::write(&codex, script).unwrap();
    let mut perms = std::fs::metadata(&codex).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&codex, perms).unwrap();

    env.cmd().arg("login").assert().success();

    assert_eq!(std::fs::read_to_string(profile_capture).unwrap(), ping_body);
    assert_eq!(
        std::fs::read_to_string(args_capture)
            .unwrap()
            .lines()
            .collect::<Vec<_>>(),
        ["--profile", "ping", "exec", "--json", "say ok"]
    );
    let probe_home = std::fs::read_to_string(home_capture).unwrap();
    assert!(
        probe_home.contains("/probe/"),
        "probe CODEX_HOME should live under state probe dir: {probe_home}"
    );
}

#[test]
fn health_fast_skips_ping_profile_validation() {
    let env = TestEnv::new_empty();
    env.seed_account("work", TEST_AUTH);

    env.cmd()
        .args(["account", "health", "--fast", "--format", "json"])
        .assert()
        .success();
}

#[test]
fn health_fast_json_token_is_bare_unknown() {
    let env = TestEnv::new_empty();
    env.seed_account("work", TEST_AUTH);

    let out = env
        .cmd()
        .args(["account", "health", "--fast", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let arr = value.as_array().unwrap();
    assert_eq!(arr[0]["token"], "unknown");
}

#[test]
fn health_fast_json_uses_cache_when_present() {
    let env = TestEnv::new_empty();
    env.seed_account("work", TEST_AUTH);

    let cache = env
        .state_session_root()
        .join("cache")
        .join("quota")
        .join("work.json");
    std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
    std::fs::write(
        &cache,
        r#"{
    "fetched_at_unix": 1700000000,
    "ttl_secs": 30,
    "body": {
        "kind": "ok",
        "five_hour": {"percent_left": 80.0, "reset_at_unix": 1710000000},
        "weekly": {"percent_left": 70.0, "reset_at_unix": 1710500000}
    }
}"#,
    )
    .unwrap();

    let out = env
        .cmd()
        .args(["account", "health", "--fast", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let arr = value.as_array().unwrap();
    assert_eq!(arr[0]["account"], "work");
    assert_eq!(arr[0]["status"], "cache only");
    assert!(arr[0]["score"].is_number());
    assert_eq!(arr[0]["fetched-at-unix"], 1_700_000_000_u64);
}

#[test]
fn health_fast_json_clamps_out_of_range_cache_percentages() {
    // The cache reader (`read_quota_from_cache`) is a separate production
    // `quota::Window` construction site from `parse_window`; it must clamp
    // `percent_left` to [0, 100] too. A malformed cache with 150.0 / -20.0
    // must surface as 100.0 / 0.0 in the scoring view.
    let env = TestEnv::new_empty();
    env.seed_account("work", TEST_AUTH);

    let cache = env
        .state_session_root()
        .join("cache")
        .join("quota")
        .join("work.json");
    std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
    std::fs::write(
        &cache,
        r#"{
    "fetched_at_unix": 1700000000,
    "ttl_secs": 30,
    "body": {
        "kind": "ok",
        "five_hour": {"percent_left": 150.0, "reset_at_unix": 1710000000},
        "weekly": {"percent_left": -20.0, "reset_at_unix": 1710500000}
    }
}"#,
    )
    .unwrap();

    let out = env
        .cmd()
        .args(["account", "health", "--fast", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let arr = value.as_array().unwrap();
    assert_eq!(arr[0]["account"], "work");
    assert_eq!(
        arr[0]["scoring"]["five-hour-pct"].as_f64(),
        Some(100.0),
        "above-100 cache percent_left must clamp to 100"
    );
    assert_eq!(
        arr[0]["scoring"]["weekly-pct"].as_f64(),
        Some(0.0),
        "below-0 cache percent_left must clamp to 0"
    );
}

fn install_default_recipe_with_ping(env: &TestEnv) {
    std::fs::write(
        env.wrapper_user_config_path(),
        "[config-recipe]\ndefault = \"default\"\n",
    )
    .unwrap();
    env.install_config_recipe(
        "default",
        "config-layers:\n  - base\nprofile-files:\n  - ping\n",
        &[("base", "model = \"gpt-5.4\"\n")],
    );
    env.write_profile_file(
        "ping",
        "model = \"gpt-5.4-mini\"\nmodel_reasoning_effort = \"minimal\"\n",
    );
}

#[test]
fn account_health_fails_fast_on_old_codex() {
    let env = TestEnv::new_empty();
    env.seed_account("work", TEST_AUTH);
    install_default_recipe_with_ping(&env);
    env.make_fake_codex_with_version("codex 0.133.0", FakeCodexBehavior::AssertNotInvoked);

    env.cmd()
        .args(["account", "health"])
        .assert()
        .failure()
        .code(78)
        .stderr(predicate::str::contains("0.133.0"))
        .stderr(predicate::str::contains("0.134.0"))
        .stderr(predicate::str::contains("docs/upstream-codex.md §F6c"))
        .stderr(predicate::str::contains("fake codex normal argv path invoked").not());
}

#[test]
fn login_fails_fast_on_old_codex() {
    let env = TestEnv::new();
    install_default_recipe_with_ping(&env);
    env.make_fake_codex_with_version("codex 0.133.0", FakeCodexBehavior::AssertNotInvoked);

    env.cmd()
        .arg("login")
        .assert()
        .failure()
        .code(78)
        .stderr(predicate::str::contains("0.133.0"))
        .stderr(predicate::str::contains("0.134.0"))
        .stderr(predicate::str::contains("docs/upstream-codex.md §F6c"))
        .stderr(predicate::str::contains("fake codex normal argv path invoked").not());
}
