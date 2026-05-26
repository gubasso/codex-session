//! Live integration tests for the WHAM usage API.
//!
//! These tests hit the real `https://chatgpt.com/backend-api/wham/usage`
//! endpoint and validate the response against the specification in
//! `docs/wham-usage-api-spec.md`.
//!
//! **Tier:** Live. Selected by nextest `[profile.live]` via
//! `binary(/live/)`. Excluded from the `pre-push` profile by the
//! `kind(test) - binary(/live/)` filter subtraction. Never runs in
//! any git hook.
//!
//! **Invocation:**
//!
//! ```sh
//! CODEX_SESSION_LIVE_TESTS=1 just test-live
//! ```
//!
//! **Prerequisites:**
//! - At least one account with valid OAuth credentials in the real
//!   registry (run `codex-session account add <name>` first).
//! - Network access to chatgpt.com.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(missing_docs)]

use std::time::{SystemTime, UNIX_EPOCH};

fn live_tests_enabled() -> bool {
    std::env::var("CODEX_SESSION_LIVE_TESTS")
        .ok()
        .is_some_and(|v| v == "1" || v == "true")
}

#[test]
fn live_quota_parses_successfully() {
    if !live_tests_enabled() {
        eprintln!(
            "skipped: set CODEX_SESSION_LIVE_TESTS=1 to run \
            live tests (or use `just test-live`)"
        );
        return;
    }

    let output = assert_cmd::Command::cargo_bin("codex-session")
        .unwrap()
        .args(["account", "quota", "--live", "--format", "json"])
        .output()
        .expect("failed to execute codex-session");

    assert!(
        output.status.success(),
        "account quota --live failed (exit {}).\n\
        stderr: {}\n\
        Upstream WHAM API may have changed. \
        See docs/wham-usage-api-spec.md §4 and run §7 re-verification recipe.",
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stderr),
    );

    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|err| {
        panic!(
            "failed to parse JSON output: {err}\n\
            stdout: {}\n\
            Upstream WHAM API may have changed. \
            See docs/wham-usage-api-spec.md §4 and run §7 re-verification recipe.",
            String::from_utf8_lossy(&output.stdout),
        );
    });

    let five_hour_pct = value["five-hour"]["percent-left"].as_f64();
    assert!(
        five_hour_pct.is_some(),
        "five-hour.percent-left missing or not a number. \
        Upstream WHAM API may have changed. \
        See docs/wham-usage-api-spec.md §4 and run §7 re-verification recipe.\n\
        Raw output: {value}",
    );
    let pct = five_hour_pct.unwrap();
    assert!(
        (0.0..=100.0).contains(&pct),
        "five-hour.percent-left out of range: {pct}",
    );

    let weekly_pct = value["weekly"]["percent-left"].as_f64();
    assert!(
        weekly_pct.is_some(),
        "weekly.percent-left missing or not a number. \
        Upstream WHAM API may have changed. \
        See docs/wham-usage-api-spec.md §4 and run §7 re-verification recipe.\n\
        Raw output: {value}",
    );
    let pct = weekly_pct.unwrap();
    assert!(
        (0.0..=100.0).contains(&pct),
        "weekly.percent-left out of range: {pct}",
    );
}

#[test]
fn live_quota_reset_at_is_future() {
    if !live_tests_enabled() {
        eprintln!(
            "skipped: set CODEX_SESSION_LIVE_TESTS=1 to run \
            live tests (or use `just test-live`)"
        );
        return;
    }

    let output = assert_cmd::Command::cargo_bin("codex-session")
        .unwrap()
        .args(["account", "quota", "--live", "--format", "json"])
        .output()
        .expect("failed to execute codex-session");

    assert!(output.status.success(), "account quota --live failed");

    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let five_hour_reset = value["five-hour"]["reset-at-unix"].as_u64();
    assert!(
        five_hour_reset.is_some_and(|ts| ts > 0),
        "five-hour.reset-at-unix missing or zero. \
        Upstream WHAM API may have changed. \
        See docs/wham-usage-api-spec.md §4 and run §7 re-verification recipe.\n\
        Raw output: {value}",
    );
    let ts = five_hour_reset.unwrap();
    assert!(
        ts > now.saturating_sub(86400),
        "five-hour.reset-at-unix ({ts}) is more than 24h in the past \
        (now={now}). See docs/wham-usage-api-spec.md §5d for reset-time \
        field mappings.",
    );

    let weekly_reset = value["weekly"]["reset-at-unix"].as_u64();
    assert!(
        weekly_reset.is_some_and(|ts| ts > 0),
        "weekly.reset-at-unix missing or zero. \
        Upstream WHAM API may have changed. \
        See docs/wham-usage-api-spec.md §4 and run §7 re-verification recipe.\n\
        Raw output: {value}",
    );
}
