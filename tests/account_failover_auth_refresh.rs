//! Tests for the `retry_same` mechanism: `401` detected in child output →
//! token refresh succeeds → same account retried (no cooldown, no rotation).
#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

mod support;

use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

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

fn refresh_success() -> String {
    serde_json::json!({
        "access_token": "new-token",
        "refresh_token": "new-rt",
        "token_type": "Bearer",
        "expires_in": 864_000
    })
    .to_string()
}

const OAUTH_AUTH: &str =
    r#"{"tokens":{"access_token":"old-token","refresh_token":"old-rt","account_id":"acct-1"}}"#;

/// 401 in child output → token refresh succeeds → same account retried → child succeeds.
/// No cooldown written, no account rotation.
#[tokio::test]
async fn auth_failure_refresh_success_retries_same_account() {
    let env = TestEnv::new_empty();

    // "work" has higher quota — selected first.
    env.seed_account("work", OAUTH_AUTH);
    env.write_quota_cache("work", &quota_cache(90.0, 90.0));

    // "personal" has lower quota — should NOT be reached.
    env.seed_account("personal", OAUTH_AUTH);
    env.write_quota_cache("personal", &quota_cache(80.0, 80.0));

    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .and(body_string_contains("refresh_token"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(refresh_success(), "application/json"),
        )
        .expect(1)
        .mount(&server)
        .await;

    let token_url = format!("{}/oauth/token", server.uri());

    let assert = env
        .cmd()
        .env(
            "CODEX_SESSION_CHILD_BIN",
            fixture_path("fake-401-then-ok.sh"),
        )
        .env("CODEX_SESSION_TOKEN_ENDPOINT", &token_url)
        .args([
            "-v",
            "--account",
            "auto",
            "--max-retries",
            "2",
            "exec",
            "trigger 401 then ok",
        ])
        .assert()
        .success();

    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();

    // First attempt emits the 401 marker, second emits the ok marker.
    let markers_401: Vec<_> = stderr
        .lines()
        .filter(|l| l.contains("marker:codex-session-fake-401-then-ok"))
        .collect();
    let markers_ok: Vec<_> = stderr
        .lines()
        .filter(|l| l.contains("marker:codex-session-ok"))
        .collect();
    assert_eq!(markers_401.len(), 1, "expected one 401 attempt");
    assert_eq!(markers_ok.len(), 1, "expected one success attempt");

    // Both attempts used the SAME account, proving retry_same worked.
    let home_401 = markers_401[0].split("home=").nth(1).unwrap_or("missing");
    let home_ok = markers_ok[0].split("home=").nth(1).unwrap_or("missing");
    assert_eq!(
        home_401, home_ok,
        "retry_same must reuse the exact same CODEX_HOME"
    );

    // Determine which account was picked.
    let picked = if home_401.contains("/accounts/work/") {
        "work"
    } else {
        "personal"
    };

    // No cooldown written — refresh succeeded, so no rotation.
    assert!(
        !env.named_account_root("work")
            .join("cooldown.json")
            .exists(),
        "no cooldown should be written when refresh succeeds"
    );
    assert!(
        !env.named_account_root("personal")
            .join("cooldown.json")
            .exists(),
        "personal should never be touched"
    );

    // Refreshed tokens persisted to disk.
    let seed = std::fs::read_to_string(env.named_account_auth_seed(picked)).unwrap();
    assert!(
        seed.contains("new-token"),
        "auth should have refreshed access_token: {seed}"
    );

    // Log verification.
    let log_file = latest_log_file(&env.state_home.join("codex-session"));
    let logs = std::fs::read_to_string(log_file).unwrap();
    assert!(
        logs.contains("\"op\":\"failover.match\""),
        "missing failover.match log"
    );
    assert!(
        logs.contains("\"op\":\"token_refresh.ok\""),
        "missing token_refresh.ok log"
    );
    assert!(
        !logs.contains("\"op\":\"account.switch\""),
        "account.switch should NOT appear — same account retried"
    );
    assert!(
        !logs.contains("\"op\":\"cooldown.write\""),
        "cooldown.write should NOT appear"
    );
}

/// 401 in child output → token refresh fails (no `refresh_token` in auth) →
/// cooldown written → account rotation. This confirms the fallback path.
#[tokio::test]
async fn auth_failure_refresh_fails_rotates_to_next_account() {
    let env = TestEnv::new_empty();

    // Auth WITHOUT refresh_token → refresh will fail with NoRefreshToken.
    let no_rt_auth = r#"{"tokens":{"access_token":"test"}}"#;
    env.seed_account("work", no_rt_auth);
    env.write_quota_cache("work", &quota_cache(90.0, 90.0));

    env.seed_account("personal", no_rt_auth);
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
        .filter_map(|l| l.strip_prefix("marker:codex-session-fake-401 home="))
        .collect();
    assert_eq!(homes.len(), 2, "expected two attempts");
    assert_ne!(homes[0], homes[1], "should rotate to different account");

    assert!(
        env.named_account_root("work")
            .join("cooldown.json")
            .exists(),
        "cooldown should be written for work"
    );

    let log_file = latest_log_file(&env.state_home.join("codex-session"));
    let logs = std::fs::read_to_string(log_file).unwrap();
    assert!(logs.contains("\"op\":\"account.switch\""));
    assert!(logs.contains("401 detected"));
}
