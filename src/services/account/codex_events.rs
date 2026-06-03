//! Best-effort reader for the subset of `codex exec --json` JSONL events the
//! wrapper needs to classify failures. Mirrors the line-walk in
//! `crate::services::session::thread_index::extract_thread_id`. Pure: no I/O.
#![allow(dead_code)]

use serde_json::Value;

#[derive(Debug, Clone, serde::Deserialize, Default)]
pub(crate) struct RateLimitWindow {
    #[serde(default)]
    pub used_percent: Option<f64>,
    #[serde(default)]
    pub window_minutes: Option<u64>,
    #[serde(default)]
    pub resets_in_seconds: Option<u64>,
    #[serde(default)]
    pub resets_at: Option<i64>,
}

#[derive(Debug, Clone, serde::Deserialize, Default)]
pub(crate) struct RateLimitSnapshot {
    #[serde(default)]
    pub primary: Option<RateLimitWindow>,
    #[serde(default)]
    pub secondary: Option<RateLimitWindow>,
    #[serde(default)]
    pub plan_type: Option<String>,
    #[serde(default)]
    pub rate_limit_reached_type: Option<String>,
}

/// Terminal error view from a `turn.failed` (or top-level `error`) event.
#[derive(Debug, Clone, Default)]
pub(crate) struct TurnError {
    pub message: String,
    pub error_code: Option<String>,
    pub retry_after_seconds: Option<u64>,
    pub http_status: Option<u32>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct EventSummary {
    pub last_rate_limits: Option<RateLimitSnapshot>,
    pub turn_error: Option<TurnError>,
}

const KNOWN_ERROR_CODES: [&str; 3] = [
    "usage_limit_reached",
    "usage_limit_exceeded",
    "context_window_exceeded",
];

pub(crate) fn scan_events(stdout: &[u8]) -> EventSummary {
    let mut summary = EventSummary::default();

    for line in stdout.split(|b| *b == b'\n') {
        if line.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_slice::<Value>(line) else {
            continue;
        };
        match value.get("type").and_then(Value::as_str) {
            Some("token_count") => {
                if let Some(rate_limits) = value.get("rate_limits") {
                    if rate_limits.is_null() {
                        continue;
                    }
                    if let Ok(snapshot) =
                        serde_json::from_value::<RateLimitSnapshot>(rate_limits.clone())
                    {
                        summary.last_rate_limits = Some(snapshot);
                    }
                }
            }
            Some("turn.failed" | "error") => {
                summary.turn_error = Some(extract_turn_error(&value));
            }
            _ => {}
        }
    }

    summary
}

fn extract_turn_error(value: &Value) -> TurnError {
    let message = find_message(value).unwrap_or_default();
    let error_code = find_error_code(value, &message);
    let retry_after_seconds = find_retry_after_seconds(value);
    let http_status = find_http_status(value);
    TurnError {
        message,
        error_code,
        retry_after_seconds,
        http_status,
    }
}

fn find_message(value: &Value) -> Option<String> {
    if let Some(message) = value.get("message").and_then(Value::as_str) {
        return Some(message.to_owned());
    }
    find_key(value, "message")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn find_error_code(value: &Value, message: &str) -> Option<String> {
    for key in ["error_code", "code", "type"] {
        if let Some(code) = find_key(value, key)
            .and_then(Value::as_str)
            .and_then(match_known_error_code)
        {
            return Some(code.to_owned());
        }
    }

    if let Some(code) = match_known_error_code(message) {
        return Some(code.to_owned());
    }

    find_known_error_code_in_tree(value).map(ToOwned::to_owned)
}

fn find_known_error_code_in_tree(value: &Value) -> Option<&'static str> {
    match value {
        Value::String(text) => match_known_error_code(text),
        Value::Array(items) => items.iter().find_map(find_known_error_code_in_tree),
        Value::Object(map) => map.values().find_map(find_known_error_code_in_tree),
        _ => None,
    }
}

fn match_known_error_code(text: &str) -> Option<&'static str> {
    KNOWN_ERROR_CODES
        .iter()
        .copied()
        .find(|code| text.contains(code))
}

fn find_retry_after_seconds(value: &Value) -> Option<u64> {
    for key in ["retry_after_seconds", "retry_after"] {
        if let Some(seconds) = find_key(value, key).and_then(value_to_seconds) {
            return Some(seconds);
        }
    }
    None
}

fn find_http_status(value: &Value) -> Option<u32> {
    for key in ["http_status", "http_status_code"] {
        if let Some(status) = find_key(value, key).and_then(value_to_u32_checked) {
            return Some(status);
        }
    }
    None
}

fn find_key<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    match value {
        Value::Object(map) => map
            .get(key)
            .or_else(|| map.values().find_map(|child| find_key(child, key))),
        Value::Array(items) => items.iter().find_map(|child| find_key(child, key)),
        _ => None,
    }
}

fn value_to_seconds(value: &Value) -> Option<u64> {
    match value {
        Value::Number(number) => number
            .as_u64()
            .or_else(|| number.as_f64().and_then(f64_floor_to_u64_saturating)),
        Value::String(text) => string_to_seconds(text),
        _ => None,
    }
}

fn string_to_seconds(text: &str) -> Option<u64> {
    text.parse::<u64>().ok().or_else(|| {
        text.parse::<f64>()
            .ok()
            .and_then(f64_floor_to_u64_saturating)
    })
}

#[allow(clippy::cast_precision_loss)]
fn f64_floor_to_u64_saturating(value: f64) -> Option<u64> {
    if !value.is_finite() || value.is_sign_negative() {
        return None;
    }
    let floored = value.floor();
    if floored >= u64::MAX as f64 {
        return Some(u64::MAX);
    }
    floored.to_string().parse::<u64>().ok()
}

fn value_to_u32_checked(value: &Value) -> Option<u32> {
    match value {
        Value::Number(number) => {
            if let Some(raw) = number.as_u64() {
                return u32::try_from(raw).ok();
            }
            number.as_i64().and_then(|raw| u32::try_from(raw).ok())
        }
        _ => None,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::scan_events;

    #[test]
    fn scan_events_reads_snapshot_and_turn_error() {
        let token_count = serde_json::json!({
            "type": "token_count",
            "rate_limits": {
                "primary": {
                    "used_percent": 99.5,
                    "window_minutes": 300,
                    "resets_in_seconds": 120,
                },
                "secondary": {"used_percent": 50.0, "resets_at": 1_700_000_000},
                "plan_type": "plus",
                "rate_limit_reached_type": "primary",
            },
        })
        .to_string();
        let turn_failed = serde_json::json!({
            "type": "turn.failed",
            "message": "Hit your usage limit",
            "error": {
                "error_code": "usage_limit_reached",
                "retry_after": 45,
                "http_status_code": 429,
            },
        })
        .to_string();
        let stdout = format!("{token_count}\n{turn_failed}\n");

        let summary = scan_events(stdout.as_bytes());
        let snapshot = summary.last_rate_limits.unwrap();
        assert_eq!(snapshot.plan_type.as_deref(), Some("plus"));
        assert_eq!(snapshot.rate_limit_reached_type.as_deref(), Some("primary"));
        assert_eq!(
            snapshot
                .primary
                .as_ref()
                .and_then(|window| window.used_percent),
            Some(99.5)
        );
        assert_eq!(
            snapshot
                .secondary
                .as_ref()
                .and_then(|window| window.resets_at),
            Some(1_700_000_000)
        );

        let turn_error = summary.turn_error.unwrap();
        assert_eq!(turn_error.message, "Hit your usage limit");
        assert_eq!(
            turn_error.error_code.as_deref(),
            Some("usage_limit_reached")
        );
        assert_eq!(turn_error.retry_after_seconds, Some(45));
        assert_eq!(turn_error.http_status, Some(429));
    }

    #[test]
    fn keeps_last_non_null_snapshot() {
        let populated = serde_json::json!({
            "type": "token_count",
            "rate_limits": {
                "primary": {"used_percent": 88.0, "resets_in_seconds": 30},
                "rate_limit_reached_type": "primary",
            },
        })
        .to_string();
        let null_snapshot =
            serde_json::json!({"type": "token_count", "rate_limits": null}).to_string();
        let stdout = format!("{populated}\n{null_snapshot}\n");

        let summary = scan_events(stdout.as_bytes());
        let snapshot = summary.last_rate_limits.unwrap();
        assert_eq!(
            snapshot.primary.and_then(|window| window.used_percent),
            Some(88.0)
        );
    }

    #[test]
    fn ignores_unknown_fields_and_absent_rate_limits() {
        let no_limits = serde_json::json!({"type": "token_count", "ignored": true}).to_string();
        let with_unknown = serde_json::json!({
            "type": "token_count",
            "rate_limits": {
                "primary": {"used_percent": 80.0},
                "extra": {"ignored": true},
                "plan_type": "pro",
            },
            "unexpected": "value",
        })
        .to_string();
        let stdout = format!("{no_limits}\n{with_unknown}\n");

        let summary = scan_events(stdout.as_bytes());
        let snapshot = summary.last_rate_limits.unwrap();
        assert_eq!(snapshot.plan_type.as_deref(), Some("pro"));
        assert_eq!(
            snapshot.primary.and_then(|window| window.used_percent),
            Some(80.0)
        );
    }

    #[test]
    fn handles_null_and_absent_rate_limits() {
        let stdout = concat!(
            "{\"type\":\"token_count\",\"rate_limits\":null}\n",
            "{\"type\":\"token_count\"}\n"
        );

        let summary = scan_events(stdout.as_bytes());
        assert!(summary.last_rate_limits.is_none());
    }

    #[test]
    fn skips_malformed_trailing_jsonl_lines() {
        let token_count = serde_json::json!({
            "type": "token_count",
            "rate_limits": {"primary": {"used_percent": 77.0}},
        })
        .to_string();
        let turn_failed = serde_json::json!({
            "type": "turn.failed",
            "message": "usage_limit_exceeded",
            "error": {"retry_after": "12.9"},
        })
        .to_string();
        // A truncated final line (no closing brace) must fail to parse and be skipped.
        let stdout = format!("{token_count}\n{turn_failed}\n{{\"type\":\"turn.failed\"");

        let summary = scan_events(stdout.as_bytes());
        assert_eq!(
            summary
                .last_rate_limits
                .and_then(|snapshot| snapshot.primary)
                .and_then(|window| window.used_percent),
            Some(77.0)
        );
        let turn_error = summary.turn_error.unwrap();
        assert_eq!(
            turn_error.error_code.as_deref(),
            Some("usage_limit_exceeded")
        );
        assert_eq!(turn_error.retry_after_seconds, Some(12));
    }

    #[test]
    fn extracts_fallback_shapes_and_context_window_error() {
        let error_event = serde_json::json!({
            "type": "error",
            "payload": {
                "message": "context_window_exceeded while streaming",
                "details": {
                    "code": "context_window_exceeded",
                    "retry_after": "7.8",
                    "http_status": 503,
                },
            },
        })
        .to_string();
        let turn_failed = serde_json::json!({
            "type": "turn.failed",
            "details": {
                "message": "usage_limit_exceeded",
                "retry_after_seconds": 3.5,
                "http_status_code": 429,
            },
        })
        .to_string();
        let stdout = format!("{error_event}\n{turn_failed}\n");

        let summary = scan_events(stdout.as_bytes());
        let turn_error = summary.turn_error.unwrap();
        assert_eq!(turn_error.message, "usage_limit_exceeded");
        assert_eq!(
            turn_error.error_code.as_deref(),
            Some("usage_limit_exceeded")
        );
        assert_eq!(turn_error.retry_after_seconds, Some(3));
        assert_eq!(turn_error.http_status, Some(429));
    }

    #[test]
    fn extracts_context_window_from_top_level_error_shape() {
        let stdout = serde_json::json!({
            "type": "error",
            "payload": {
                "message": "context_window_exceeded while streaming",
                "details": {
                    "code": "context_window_exceeded",
                    "retry_after": "7.8",
                    "http_status": 503,
                },
            },
        })
        .to_string();

        let summary = scan_events(stdout.as_bytes());
        let turn_error = summary.turn_error.unwrap();
        assert_eq!(
            turn_error.error_code.as_deref(),
            Some("context_window_exceeded")
        );
        assert_eq!(turn_error.retry_after_seconds, Some(7));
        assert_eq!(turn_error.http_status, Some(503));
    }
}
