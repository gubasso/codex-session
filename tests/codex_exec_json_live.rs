//! Live verification of the `codex exec --json` event-stream schema.
//!
//! Answers the question the error-classification work depends on: does
//! `codex exec --json` emit `token_count` events carrying a non-null
//! `rate_limits` snapshot in exec mode, or is it null/absent (openai/codex
//! issue #14728)? Also pins the `RateLimitWindow` field shapes
//! (`used_percent`, `resets_in_seconds`/`resets_at`) so upstream drift in the
//! rate-limit snapshot surfaces here. Pinning the concrete `turn.failed` error
//! payload/discriminant shape is deferred to Round 2, which extends this test
//! once a live error run is observed.
//!
//! Makes one real, billable `codex` API call via `codex-session exec` with a
//! harmless no-op prompt. Does NOT hard-fail when the run is itself
//! rate-limited — that case is captured and reported.
//!
//! Tier: Live. Selected by nextest `[profile.live]` via `binary(/live/)`.
//! Invocation: `CODEX_SESSION_LIVE_TESTS=1 just test-live` (delegates to the
//! `cargo-nextest-live` pre-commit hook), or directly for an ad-hoc focused run:
//! `CODEX_SESSION_LIVE_TESTS=1 cargo nextest run --profile live -E 'binary(codex_exec_json_live)'`.
//!
//! Prerequisites: an account with valid OAuth credentials + quota; network.
//!
//! Verdict lines to read in the output:
//!   [codex-exec-json] event types observed: {...}
//!   [codex-exec-json] `RATE_LIMITS_IN_EXEC_MODE`: populated | null | absent |
//!                                                  no-token_count-event
//!   [codex-exec-json] `RUN_RATE_LIMITED`: true | false

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(missing_docs)]

use std::collections::BTreeSet;

const NOOP_PROMPT: &str = concat!(
    "Reply with exactly the two characters: OK. Do not run any commands, ",
    "do not use any tools, do not read or write any files.",
);

fn live_tests_enabled() -> bool {
    std::env::var("CODEX_SESSION_LIVE_TESTS")
        .ok()
        .is_some_and(|v| v == "1" || v == "true")
}

/// Recursively search a JSON tree for the first value under `key`.
fn find_key<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(found) = map.get(key) {
                return Some(found);
            }
            map.values().find_map(|v| find_key(v, key))
        }
        serde_json::Value::Array(arr) => arr.iter().find_map(|v| find_key(v, key)),
        _ => None,
    }
}

fn looks_rate_limited(haystack: &str) -> bool {
    let h = haystack.to_ascii_lowercase();
    h.contains("usage_limit_reached")
        || h.contains("usage_limit_exceeded")
        || h.contains("hit your usage limit")
        || h.contains("rate_limit")
        || h.contains("rate limit")
        || h.contains("too many requests")
        || h.contains("out of credits")
        || h.contains(" 429")
        || h.contains("\"429\"")
}

fn assert_window_shape(label: &str, window: &serde_json::Value) {
    assert!(
        window.is_object(),
        "rate_limits.{label} is not an object: {window}"
    );
    let used = window
        .get("used_percent")
        .and_then(serde_json::Value::as_f64);
    assert!(
        used.is_some(),
        "rate_limits.{label}.used_percent missing or not a number: {window}. \
RateLimitWindow shape changed upstream — update codex_events.rs + docs.",
    );
    let has_reset = window
        .get("resets_in_seconds")
        .is_some_and(serde_json::Value::is_number)
        || window
            .get("resets_at")
            .is_some_and(serde_json::Value::is_number);
    assert!(
        has_reset,
        "rate_limits.{label} carries neither numeric `resets_in_seconds` nor \
`resets_at`: {window}. Reset-timing field changed upstream.",
    );
}

#[test]
fn live_exec_json_event_schema() {
    if !live_tests_enabled() {
        eprintln!(
            "skipped: set CODEX_SESSION_LIVE_TESTS=1 to run live tests \
(or use `just test-live`)"
        );
        return;
    }

    let output = assert_cmd::Command::cargo_bin("codex-session")
        .unwrap()
        .args(["exec", "--json", NOOP_PROMPT])
        .write_stdin("")
        .output()
        .expect("failed to execute codex-session");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let events: Vec<serde_json::Value> = stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .collect();

    let run_rate_limited = looks_rate_limited(&stdout) || looks_rate_limited(&stderr);

    // A rate-limited run is captured and reported, not hard-failed (see the
    // module doc comment). If the backend 429'd before any JSONL events were
    // emitted, record the verdict and return rather than tripping the
    // empty-events assertion below.
    if events.is_empty() && run_rate_limited {
        eprintln!("[codex-exec-json] event types observed: {{}}");
        eprintln!("[codex-exec-json] RUN_RATE_LIMITED: {run_rate_limited}");
        eprintln!("[codex-exec-json] RATE_LIMITS_IN_EXEC_MODE: no-token_count-event");
        return;
    }

    assert!(
        !events.is_empty(),
        "no JSONL events parsed from `codex-session exec --json`.\n\
Was --json honored? Is an account authed with quota?\n\
exit: {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status.code(),
    );

    let types: BTreeSet<&str> = events
        .iter()
        .filter_map(|e| e.get("type").and_then(serde_json::Value::as_str))
        .collect();
    eprintln!("[codex-exec-json] event types observed: {types:?}");
    eprintln!("[codex-exec-json] RUN_RATE_LIMITED: {run_rate_limited}");

    let token_count = events
        .iter()
        .rev()
        .find(|e| e.get("type").and_then(serde_json::Value::as_str) == Some("token_count"));
    let rate_limits = token_count.and_then(|e| find_key(e, "rate_limits"));

    let verdict = match rate_limits {
        Some(v) if v.is_null() => "null",
        Some(_) => "populated",
        None if token_count.is_some() => "absent-from-token_count",
        None => "no-token_count-event",
    };
    eprintln!("[codex-exec-json] RATE_LIMITS_IN_EXEC_MODE: {verdict}");
    if let Some(rl) = rate_limits {
        eprintln!("[codex-exec-json] rate_limits snapshot: {rl}");
    }

    // `token_count` is NOT guaranteed in exec mode: a successful run may
    // carry usage only on `turn.completed.usage` and emit no `token_count`
    // event at all (verified live on codex 0.135.0; see upstream-codex.md
    // §F9). Accept that shape as an intact schema; only fail when the stream
    // has none of the recognized terminal signals.
    let turn_completed_usage = events.iter().any(|e| {
        e.get("type").and_then(serde_json::Value::as_str) == Some("turn.completed")
            && e.get("usage").is_some_and(serde_json::Value::is_object)
    });
    assert!(
        token_count.is_some() || turn_completed_usage || run_rate_limited,
        "event stream had neither a `token_count` event, a `turn.completed` \
event with a `usage` object, nor a recognized rate/usage-limit signal — \
schema may have drifted.\n\
event types: {types:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
    );

    if let Some(rl) = rate_limits.filter(|rl| !rl.is_null()) {
        if let Some(primary) = rl.get("primary").filter(|v| !v.is_null()) {
            assert_window_shape("primary", primary);
        }
        if let Some(secondary) = rl.get("secondary").filter(|v| !v.is_null()) {
            assert_window_shape("secondary", secondary);
        }
        assert!(
            rl.get("primary").is_some_and(|v| !v.is_null())
                || rl.get("secondary").is_some_and(|v| !v.is_null()),
            "rate_limits populated but has neither primary nor secondary window: {rl}",
        );
    }
}
