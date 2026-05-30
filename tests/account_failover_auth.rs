#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

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

#[test]
fn auto_retry_rotates_accounts_on_auth_failure() {
    let env = TestEnv::new_empty();
    env.seed_account("personal", "{\"tokens\":{\"access_token\":\"test\"}}\n");
    env.seed_account("work", "{\"tokens\":{\"access_token\":\"test\"}}\n");
    env.write_quota_cache("work", &quota_cache(90.0, 90.0));
    env.write_quota_cache("personal", &quota_cache(80.0, 80.0));

    let assert = env
        .cmd()
        .env("CODEX_SESSION_CHILD_BIN", fixture_path("fake-401.sh"))
        .args([
            "-v",
            "--account",
            "auto",
            "--max-retries",
            "2",
            "exec",
            "trigger 401",
        ])
        .assert()
        .failure()
        .code(75);

    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    let homes: Vec<_> = stderr
        .lines()
        .filter_map(|line| line.strip_prefix("marker:codex-session-fake-401 home="))
        .map(ToOwned::to_owned)
        .collect();
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

    let log_file = latest_log_file(&env.state_home.join("codex-session"));
    let logs = std::fs::read_to_string(log_file).unwrap();
    for op in [
        "\"op\":\"failover.match\"",
        "\"op\":\"account.switch\"",
        "\"op\":\"cooldown.write\"",
        "\"op\":\"retry.exhausted\"",
    ] {
        assert!(logs.contains(op), "missing log op {op}");
    }
    assert!(
        logs.contains("401 detected"),
        "expected 401 cooldown reason in logs"
    );
}
