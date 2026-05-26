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
        .unwrap_or_else(|| unreachable!("no log file found"))
}

/// Expired JWT: exp=1700000000 (2023-11-14, well in the past)
const EXPIRED_AUTH: &str =
    r#"{"tokens":{"access_token":"eyJhbGciOiJub25lIn0.eyJleHAiOjE3MDAwMDAwMDB9."}}"#;

/// Valid JWT: exp=4102444800 (2099-12-31, far future)
const VALID_AUTH: &str =
    r#"{"tokens":{"access_token":"eyJhbGciOiJub25lIn0.eyJleHAiOjQxMDI0NDQ4MDB9."}}"#;

#[test]
fn expired_token_account_skipped_by_selector() {
    let env = TestEnv::new_empty();

    // "expired" has higher quota — would be selected first without the pre-check.
    env.seed_account("expired", EXPIRED_AUTH);
    env.write_quota_cache("expired", &quota_cache(99.0, 99.0));

    env.seed_account("valid", VALID_AUTH);
    env.write_quota_cache("valid", &quota_cache(80.0, 80.0));

    let assert = env
        .cmd()
        .env("CODEX_SESSION_CHILD_BIN", fixture_path("echo-env.sh"))
        .args(["-vv", "--account", "auto", "exec", "test"])
        .assert()
        .success();

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains("/accounts/valid/groups/"),
        "expected 'valid' account selected, got stdout:\n{stdout}"
    );
    assert!(
        !stdout.contains("/accounts/expired/groups/"),
        "expired account should not appear in CODEX_HOME"
    );

    let log_file = latest_log_file(&env.state_home.join("codex-session"));
    let logs = std::fs::read_to_string(log_file).unwrap();
    assert!(
        logs.contains("token-expired"),
        "expected 'token-expired' reason in debug logs"
    );
}

#[test]
fn valid_token_account_not_skipped() {
    let env = TestEnv::new_empty();

    // Both accounts valid — the one with higher quota should be selected.
    env.seed_account("high", VALID_AUTH);
    env.write_quota_cache("high", &quota_cache(95.0, 95.0));

    env.seed_account("low", VALID_AUTH);
    env.write_quota_cache("low", &quota_cache(50.0, 50.0));

    let assert = env
        .cmd()
        .env("CODEX_SESSION_CHILD_BIN", fixture_path("echo-env.sh"))
        .args(["-vv", "--account", "auto", "exec", "test"])
        .assert()
        .success();

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains("/accounts/high/groups/"),
        "expected 'high' account selected by quota score, got stdout:\n{stdout}"
    );

    let log_file = latest_log_file(&env.state_home.join("codex-session"));
    let logs = std::fs::read_to_string(log_file).unwrap();
    assert!(
        !logs.contains("token-expired"),
        "no account should be skipped for token expiry"
    );
}
