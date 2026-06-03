#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use std::collections::HashSet;

use support::{TestEnv, fixture_path};

fn quota_cache(five_hour: f64, weekly: f64) -> String {
    format!(
        r#"{{
    "fetched_at_unix": 4102444800,
    "ttl_secs": 30,
    "body": {{
    "kind": "ok",
    "five_hour": {{ "percent_left": {five_hour}, "reset_at_unix": 4102448400 }},
    "weekly": {{ "percent_left": {weekly}, "reset_at_unix": 4103053200 }}
    }}
}}"#
    )
}

/// Like `quota_cache`, but with a caller-chosen five-hour `reset_at_unix` so a
/// test can pin the reset-aware credit cooldown to a known instant.
fn quota_cache_with_five_hour_reset(five_hour: f64, weekly: f64, reset_at_unix: u64) -> String {
    format!(
        r#"{{
    "fetched_at_unix": 4102444800,
    "ttl_secs": 30,
    "body": {{
    "kind": "ok",
    "five_hour": {{ "percent_left": {five_hour}, "reset_at_unix": {reset_at_unix} }},
    "weekly": {{ "percent_left": {weekly}, "reset_at_unix": 4103053200 }}
    }}
}}"#
    )
}

fn latest_log_file(dir: &std::path::Path) -> std::path::PathBuf {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("codex-session.log"))
        })
        .collect();
    entries.sort();
    entries
        .pop()
        .unwrap_or_else(|| unreachable!("entries was checked to be non-empty"))
}

fn read_cooldown(env: &TestEnv, account: &str) -> serde_json::Value {
    let path = env.named_account_root(account).join("cooldown.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn marker_homes<'a>(stderr: &'a str, prefix: &str) -> Vec<&'a str> {
    stderr
        .lines()
        .filter_map(|line| line.strip_prefix(prefix))
        .filter_map(|line| line.split_whitespace().next())
        .collect()
}

#[test]
fn auto_retry_rotates_accounts_and_writes_cooldown() {
    let env = TestEnv::new_empty();
    env.seed_account("personal", "{\"token\":\"test\"}\n");
    env.seed_account("work", "{\"token\":\"test\"}\n");
    env.write_quota_cache("work", &quota_cache(90.0, 90.0));
    env.write_quota_cache("personal", &quota_cache(80.0, 80.0));

    let assert = env
        .cmd()
        .env(
            "CODEX_SESSION_CHILD_BIN",
            fixture_path("fake-429-usage-jsonl.sh"),
        )
        .args([
            "-v",
            "--account",
            "auto",
            "--max-retries",
            "2",
            "exec",
            "trigger 429",
        ])
        .assert()
        .failure()
        .code(75);

    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    let homes = marker_homes(&stderr, "marker:codex-session-fake-429-usage home=");
    assert_eq!(
        homes.len(),
        2,
        "expected two child attempts before no-eligible"
    );
    assert_ne!(
        homes[0], homes[1],
        "retry should rotate to a different CODEX_HOME"
    );
    assert!(
        homes
            .iter()
            .any(|home| home.contains("/accounts/work/groups/")),
        "work should be attempted, got {homes:?}"
    );
    assert!(
        homes
            .iter()
            .any(|home| home.contains("/accounts/personal/groups/")),
        "personal should be attempted, got {homes:?}"
    );
    assert!(
        env.named_account_root("work")
            .join("cooldown.json")
            .exists()
    );
    assert!(
        env.named_account_root("personal")
            .join("cooldown.json")
            .exists()
    );
    assert!(stderr.contains("account: auto-selection exhausted"));
    assert!(stderr.contains("• work  rate limited (429)"));
    assert!(stderr.contains("• personal  rate limited (429)"));
    assert!(stderr.contains("back in"));
    assert!(stderr.contains("earliest available:"));
    assert!(stderr.contains("codex-session account cooldown clear --all"));

    let log_file = latest_log_file(&env.state_home.join("codex-session"));
    let logs = std::fs::read_to_string(log_file).unwrap();
    for op in [
        "\"op\":\"failover.match\"",
        "\"op\":\"account.switch\"",
        "\"op\":\"cooldown.write\"",
        "\"op\":\"retry.exhausted\"",
        "\"op\":\"command.error.account_outcome\"",
    ] {
        assert!(logs.contains(op), "missing log op {op}");
    }
    assert!(logs.contains("\"state\":\"rate_limited_429\""));
    assert!(logs.contains("\"earliest_available_at_unix\""));
}

#[test]
fn credits_rotate_and_write_reset_aware_cooldown() {
    let env = TestEnv::new_empty();
    env.seed_account("work", "{\"token\":\"test\"}\n");
    env.seed_account("personal", "{\"token\":\"test\"}\n");
    // Exhausted five-hour windows (still selectable — quota is a soft
    // penalty, not a block) with a pinned future reset: the credit cooldown
    // must expire at exactly that window reset (see retry::credit_cooldown).
    let start = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let window_reset_at = start + 1234;
    env.write_quota_cache(
        "work",
        &quota_cache_with_five_hour_reset(0.0, 27.0, window_reset_at),
    );
    env.write_quota_cache(
        "personal",
        &quota_cache_with_five_hour_reset(0.0, 27.0, window_reset_at),
    );

    let assert = env
        .cmd()
        .env(
            "CODEX_SESSION_CHILD_BIN",
            fixture_path("fake-credits-jsonl.sh"),
        )
        .args([
            "-v",
            "--account",
            "auto",
            "--max-retries",
            "2",
            "exec",
            "trigger credits",
        ])
        .assert()
        .failure()
        .code(75);

    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    let homes = marker_homes(&stderr, "marker:codex-session-fake-credits home=");
    assert_eq!(
        homes.len(),
        2,
        "credit exhaustion should rotate, not terminate"
    );
    assert_ne!(homes[0], homes[1]);

    for account in ["work", "personal"] {
        let cooldown = read_cooldown(&env, account);
        assert!(
            cooldown["reason"]
                .as_str()
                .unwrap()
                .starts_with("credits detected:"),
            "cooldown reason should be credit-specific: {cooldown}"
        );
        assert_eq!(cooldown["reset_source"], "server-reset");
        let reset_at = cooldown["reset_at_unix"].as_u64().unwrap();
        assert!(
            (window_reset_at..=window_reset_at + 30).contains(&reset_at),
            "cooldown should expire at the window reset \
            (expected ~{window_reset_at}, got {reset_at})"
        );
    }

    assert!(stderr.contains("account: auto-selection exhausted"));
    assert!(stderr.contains("• work  out of credits"));
    assert!(stderr.contains("• personal  out of credits"));
    // Clearing cooldowns does not add credits; the hint must point at the
    // window reset / top-up instead.
    assert!(stderr.contains("add credits to the workspace"));
    assert!(!stderr.contains("cooldown clear --all"));

    let log_file = latest_log_file(&env.state_home.join("codex-session"));
    let logs = std::fs::read_to_string(log_file).unwrap();
    assert!(logs.contains("\"state\":\"credit_exhausted\""));
    assert!(logs.contains("\"op\":\"account.switch\""));
    assert!(logs.contains("\"op\":\"cooldown.write\""));
}

#[test]
fn usage_limit_rotate_uses_retry_after_cooldown() {
    let env = TestEnv::new_empty();
    env.seed_account("work", "{\"token\":\"test\"}\n");
    env.seed_account("personal", "{\"token\":\"test\"}\n");
    env.write_quota_cache("work", &quota_cache(90.0, 90.0));
    env.write_quota_cache("personal", &quota_cache(80.0, 80.0));

    let start = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    env.cmd()
        .env(
            "CODEX_SESSION_CHILD_BIN",
            fixture_path("fake-429-usage-jsonl.sh"),
        )
        .args([
            "--account",
            "auto",
            "--max-retries",
            "2",
            "exec",
            "trigger 429",
        ])
        .assert()
        .failure()
        .code(75);

    let cooldown = read_cooldown(&env, "work");
    let reset_at = cooldown["reset_at_unix"].as_u64().unwrap();
    let delta = reset_at.saturating_sub(start);
    assert!(
        (44..=49).contains(&delta),
        "retry-after should drive cooldown, got delta={delta}"
    );
    assert_eq!(cooldown["reset_source"], "retry-after");
}

#[test]
fn transient_then_success_retries_same_account_without_rotation() {
    let env = TestEnv::new_empty();
    env.seed_account("work", "{\"token\":\"test\"}\n");
    env.seed_account("personal", "{\"token\":\"test\"}\n");
    env.write_quota_cache("work", &quota_cache(90.0, 90.0));
    env.write_quota_cache("personal", &quota_cache(80.0, 80.0));

    let assert = env
        .cmd()
        .env(
            "CODEX_SESSION_CHILD_BIN",
            fixture_path("fake-429-transient-jsonl-then-ok.sh"),
        )
        .args([
            "-v",
            "--account",
            "auto",
            "--max-retries",
            "2",
            "exec",
            "trigger transient 429",
        ])
        .assert()
        .success();

    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    let homes = marker_homes(&stderr, "marker:codex-session-transient-then-ok home=");
    assert_eq!(
        homes.len(),
        2,
        "expected transient retry on the same account"
    );
    assert_eq!(homes[0], homes[1], "same CODEX_HOME should be reused");
    assert!(
        homes[0].contains("/accounts/work/groups/"),
        "work should remain selected, got {homes:?}"
    );
    assert!(
        !env.named_account_root("work")
            .join("cooldown.json")
            .exists()
    );
    assert!(
        !env.named_account_root("personal")
            .join("cooldown.json")
            .exists()
    );

    let log_file = latest_log_file(&env.state_home.join("codex-session"));
    let logs = std::fs::read_to_string(log_file).unwrap();
    assert!(logs.contains("\"op\":\"retry.backoff\""));
    assert!(!logs.contains("\"op\":\"account.switch\""));
}

#[test]
fn persistent_transient_caps_then_rotates() {
    let env = TestEnv::new_empty();
    env.seed_account("work", "{\"token\":\"test\"}\n");
    env.seed_account("personal", "{\"token\":\"test\"}\n");
    env.write_quota_cache("work", &quota_cache(90.0, 90.0));
    env.write_quota_cache("personal", &quota_cache(80.0, 80.0));

    let assert = env
        .cmd()
        .env(
            "CODEX_SESSION_CHILD_BIN",
            fixture_path("fake-429-transient-jsonl-always.sh"),
        )
        .args([
            "-v",
            "--account",
            "auto",
            "--max-retries",
            "2",
            "exec",
            "trigger transient 429",
        ])
        .assert()
        .failure()
        .code(75);

    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    let homes = marker_homes(
        &stderr,
        "marker:codex-session-fake-429-transient-always home=",
    );
    assert!(
        homes.len() >= 5,
        "expected repeated same-account retries before rotation, got {homes:?}"
    );
    assert!(
        homes[0].contains("/accounts/work/groups/"),
        "work should be selected first, got {homes:?}"
    );
    assert_eq!(homes[0], homes[1]);
    assert_eq!(homes[1], homes[2]);
    assert_eq!(homes[2], homes[3]);
    assert_ne!(homes[3], homes[4], "fifth attempt should rotate");
    assert!(
        env.named_account_root("work")
            .join("cooldown.json")
            .exists()
    );

    let cooldown = read_cooldown(&env, "work");
    assert_eq!(cooldown["reset_source"], "retry-after");
}

#[test]
fn unhandled_error_does_not_rotate_or_cool_down() {
    let env = TestEnv::new_empty();
    env.seed_account("work", "{\"token\":\"test\"}\n");
    env.seed_account("personal", "{\"token\":\"test\"}\n");
    env.write_quota_cache("work", &quota_cache(90.0, 90.0));
    env.write_quota_cache("personal", &quota_cache(80.0, 80.0));

    let assert = env
        .cmd()
        .env(
            "CODEX_SESSION_CHILD_BIN",
            fixture_path("fake-unhandled-jsonl.sh"),
        )
        .args(["-v", "--account", "auto", "exec", "trigger unhandled"])
        .assert()
        .failure()
        .code(1);

    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    let homes = marker_homes(&stderr, "marker:codex-session-fake-unhandled home=");
    assert_eq!(homes.len(), 1, "unhandled errors must not rotate");
    assert!(stderr.contains("codex returned an unhandled error"));
    assert!(stderr.contains("context-window-exceeded: context window exceeded for request"));
    assert!(stderr.contains("codex-session.log"));
    assert!(
        !env.named_account_root("work")
            .join("cooldown.json")
            .exists()
    );
    assert!(
        !env.named_account_root("personal")
            .join("cooldown.json")
            .exists()
    );

    let log_file = latest_log_file(&env.state_home.join("codex-session"));
    let logs = std::fs::read_to_string(log_file).unwrap();
    assert!(logs.contains("\"op\":\"codex.error.unhandled\""));
    assert!(logs.contains("context window exceeded for request"));
    assert!(!logs.contains("\"op\":\"account.switch\""));
}

#[test]
fn auto_failover_never_recycles_across_three_accounts() {
    let env = TestEnv::new_empty();
    env.seed_account("alpha", "{\"token\":\"test\"}\n");
    env.seed_account("beta", "{\"token\":\"test\"}\n");
    env.seed_account("gamma", "{\"token\":\"test\"}\n");
    env.write_quota_cache("alpha", &quota_cache(90.0, 90.0));
    env.write_quota_cache("beta", &quota_cache(80.0, 80.0));
    env.write_quota_cache("gamma", &quota_cache(70.0, 70.0));

    let assert = env
        .cmd()
        .env(
            "CODEX_SESSION_CHILD_BIN",
            fixture_path("fake-429-usage-jsonl.sh"),
        )
        .args([
            "-v",
            "--account",
            "auto",
            "--max-retries",
            "3",
            "exec",
            "trigger 429",
        ])
        .assert()
        .failure()
        .code(75);

    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    let homes = marker_homes(&stderr, "marker:codex-session-fake-429-usage home=");

    assert_eq!(
        homes.len(),
        3,
        "expected three child attempts, got {homes:?}"
    );
    let unique: HashSet<&&str> = homes.iter().collect();
    assert_eq!(unique.len(), 3, "no account may be recycled, got {homes:?}");
    assert_ne!(homes[0], homes[1]);
    assert_ne!(homes[1], homes[2]);
    assert_ne!(homes[0], homes[2]);

    assert!(
        homes
            .iter()
            .any(|home| home.contains("/accounts/alpha/groups/")),
        "alpha should be attempted, got {homes:?}"
    );
    assert!(
        homes
            .iter()
            .any(|home| home.contains("/accounts/beta/groups/")),
        "beta should be attempted, got {homes:?}"
    );
    assert!(
        homes
            .iter()
            .any(|home| home.contains("/accounts/gamma/groups/")),
        "gamma should be attempted, got {homes:?}"
    );

    assert!(
        env.named_account_root("alpha")
            .join("cooldown.json")
            .exists()
    );
    assert!(
        env.named_account_root("beta")
            .join("cooldown.json")
            .exists()
    );
    assert!(
        env.named_account_root("gamma")
            .join("cooldown.json")
            .exists()
    );

    assert!(stderr.contains("account: auto-selection exhausted"));
    assert!(stderr.contains("• alpha  rate limited (429)"));
    assert!(stderr.contains("• beta  rate limited (429)"));
    assert!(stderr.contains("• gamma  rate limited (429)"));
}
