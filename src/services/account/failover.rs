//! Passive 429 detection for captured child output.
//!
//! What this is: a post-wait observer over captured stdout+stderr bytes.
//! What this is not: a streaming proxy or a mid-flight rotation hook.
//!
//! The wrapper scans after the child exits, then decides whether the result
//! should trigger a retry with account rotation. That keeps the existing
//! spawn path simple and avoids the much wider `PipingSpawner` refactor that
//! true streaming detection would require.
//!
//! Caam Codex rate-limit patterns. Verbatim from the caam repo
//! (`Dicklesworthstone/coding_agent_account_manager`) file
//! `internal/ratelimit/detector.go::DefaultPatterns()[ProviderCodex]`,
//! verified 2026-05-22. If caam changes, re-sync this list and the
//! unit-test matrix below.

use std::sync::LazyLock;

use regex::{Regex, RegexSet};

use super::codex_events::{EventSummary, RateLimitSnapshot, RateLimitWindow, TurnError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MatchKind {
    RateLimit,
    AuthFailure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum RateLimitClass {
    UsageLimitExhausted,
    Transient,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResetSource {
    RetryAfter,
    ServerReset,
    ParsedText,
}

impl ResetSource {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::RetryAfter => "retry-after",
            Self::ServerReset => "server-reset",
            Self::ParsedText => "try-again-text",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum Category {
    RateLimit(RateLimitClass),
    AuthFailure,
    /// Workspace credits depleted while the plan window is exhausted (see
    /// `is_credit_exhausted`). Handled like a usage-limit exhaustion: cool the
    /// account down until its window reset and rotate — NOT an unhandled error.
    CreditExhausted,
    NoRolloutFound,
    ContextWindowExceeded,
    ServerError,
    Unclassified,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) struct Classification {
    pub category: Category,
    pub reset_after_seconds: Option<u64>,
    pub reset_source: Option<ResetSource>,
    pub snippet: String,
}

const RATE_LIMIT_PATTERNS_RAW: [&str; 6] = [
    r"(?i)rate.?limit",
    r"(?i)quota.?exceeded",
    r"\b429\b",
    r"(?i)too.?many.?requests",
    r"(?i)exceeded.*rate",
    r"(?i)slow.?down",
];

/// Maximum bytes appended to the per-stream capture buffer that
/// `failover::scan` reads. Live forwarding to the parent's real stdio is
/// unbounded; only the capture buffer is capped. See ADR D9.
pub(crate) const MAX_CAPTURE_BYTES: usize = 1 << 20; // 1 MiB

#[allow(clippy::expect_used, reason = "static regex set must compile")]
static RATE_LIMIT_PATTERNS: LazyLock<RegexSet> = LazyLock::new(|| {
    RegexSet::new(RATE_LIMIT_PATTERNS_RAW).expect("static regex set must compile")
});

#[allow(dead_code)]
pub(crate) const RATE_LIMIT_PATTERN_NAMES: [&str; 6] = [
    "rate-limit",
    "quota-exceeded",
    "429",
    "too-many-requests",
    "exceeded-rate",
    "slow-down",
];

const AUTH_FAILURE_PATTERNS_RAW: [&str; 7] = [
    r"\b401\b",
    r"(?i)unauthorized",
    r"(?i)invalid.?auth",
    r"(?i)no.?auth.?credentials",
    r"(?i)invalid.?grant",
    r"(?i)token.?exchange.?error",
    r"(?i)insufficient.?permissions",
];

#[allow(clippy::expect_used, reason = "static regex set must compile")]
static AUTH_FAILURE_PATTERNS: LazyLock<RegexSet> = LazyLock::new(|| {
    RegexSet::new(AUTH_FAILURE_PATTERNS_RAW).expect("static regex set must compile")
});

#[allow(clippy::expect_used, reason = "static regex must compile")]
#[allow(dead_code)]
static USAGE_LIMIT_TEXT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(hit your usage limit|usage.?limit.?(reached|exceeded))")
        .expect("static regex must compile")
});

#[allow(clippy::expect_used, reason = "static regex must compile")]
#[allow(dead_code)]
static TRY_AGAIN_IN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)try again in\s*(\d+(?:\.\d+)?)\s*(s|ms|seconds?)")
        .expect("static regex must compile")
});

#[allow(dead_code)]
pub(crate) const AUTH_FAILURE_PATTERN_NAMES: [&str; 7] = [
    "401",
    "unauthorized",
    "invalid-auth",
    "no-auth-credentials",
    "invalid-grant",
    "token-exchange-error",
    "insufficient-permissions",
];

/// `pattern_index` is the lowest matching index within the kind-specific
/// pattern array (`RATE_LIMIT_PATTERNS_RAW` or `AUTH_FAILURE_PATTERNS_RAW`).
/// Use `pattern_name()` for the human-readable label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Match {
    pub kind: MatchKind,
    pub line_no: usize,
    pub snippet: String,
    pub pattern_index: usize,
}

pub(crate) fn scan(buf: &[u8]) -> Option<Match> {
    // Pass 1: rate-limit patterns have highest priority.
    for (idx, line) in buf.split(|byte| *byte == b'\n').enumerate() {
        let line = String::from_utf8_lossy(line);
        let matches = RATE_LIMIT_PATTERNS.matches(&line);
        if let Some(pattern_index) = matches.into_iter().next() {
            return Some(Match {
                kind: MatchKind::RateLimit,
                line_no: idx + 1,
                snippet: truncate_for_debug(&line, 256),
                pattern_index,
            });
        }
    }

    // Pass 2: auth-failure patterns are considered only when no rate-limit
    // pattern matched any line.
    for (idx, line) in buf.split(|byte| *byte == b'\n').enumerate() {
        let line = String::from_utf8_lossy(line);
        let matches = AUTH_FAILURE_PATTERNS.matches(&line);
        if let Some(pattern_index) = matches.into_iter().next() {
            return Some(Match {
                kind: MatchKind::AuthFailure,
                line_no: idx + 1,
                snippet: truncate_for_debug(&line, 256),
                pattern_index,
            });
        }
    }
    None
}

fn truncate_for_debug(line: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for ch in line.chars().take(max_chars) {
        out.push(ch);
    }
    out
}

/// Pick the highest-priority match across two streams (stderr, stdout).
/// `RateLimit` always wins over `AuthFailure`; within the same kind,
/// stderr wins (Codex emits diagnostics there).
pub(crate) fn pick_priority(stderr: Option<Match>, stdout: Option<Match>) -> Option<Match> {
    match (&stderr, &stdout) {
        (Some(e), Some(o)) => {
            if e.kind == MatchKind::RateLimit {
                stderr
            } else if o.kind == MatchKind::RateLimit {
                stdout
            } else {
                stderr
            }
        }
        (Some(_), None) => stderr,
        (None, Some(_)) => stdout,
        (None, None) => None,
    }
}

#[allow(dead_code)]
pub(crate) const fn pattern_name(matched: &Match) -> &'static str {
    match matched.kind {
        MatchKind::RateLimit => RATE_LIMIT_PATTERN_NAMES[matched.pattern_index],
        MatchKind::AuthFailure => AUTH_FAILURE_PATTERN_NAMES[matched.pattern_index],
    }
}

/// Structured-first classifier. `events` comes from
/// `codex_events::scan_events(stdout)`; `stderr`/`stdout` feed the text
/// fallback. Pure: no I/O, no sleep, no cooldown writes, no rotation.
#[allow(dead_code)]
pub(crate) fn classify(
    events: &EventSummary,
    stdout: &[u8],
    stderr: &[u8],
) -> Option<Classification> {
    let structured_present = events.turn_error.is_some() || events.last_rate_limits.is_some();
    let structured = if structured_present {
        classify_structured(events)
    } else {
        None
    };
    // A structured result is preferred only when it carries a real signal. An
    // `Unclassified` structured error (a `turn.failed` whose shape matched no
    // known category) must NOT suppress a stronger legacy text signal such as a
    // `401 Unauthorized` or a recognizable 429 in stderr/stdout — fall through to
    // the text fallback first and keep the `Unclassified` only if text finds
    // nothing.
    if let Some(classification) = &structured
        && !matches!(classification.category, Category::Unclassified)
    {
        return structured;
    }

    let Some(text_match) = pick_priority(scan(stderr), scan(stdout)) else {
        if let Some(snippet) = scan_no_rollout_text(stderr).or_else(|| scan_no_rollout_text(stdout))
        {
            return Some(Classification {
                category: Category::NoRolloutFound,
                reset_after_seconds: None,
                reset_source: None,
                snippet,
            });
        }
        if let Some(snippet) =
            scan_usage_limit_text(stderr).or_else(|| scan_usage_limit_text(stdout))
        {
            return Some(Classification {
                category: Category::RateLimit(RateLimitClass::UsageLimitExhausted),
                reset_after_seconds: parse_reset_after_seconds(&snippet),
                reset_source: parse_reset_after_seconds(&snippet).map(|_| ResetSource::ParsedText),
                snippet,
            });
        }
        // No text signal at all: fall back to the structured `Unclassified`
        // (carrying its snippet) when present, so Round 3's general handler still
        // has something to surface.
        return structured;
    };
    let category = match text_match.kind {
        MatchKind::AuthFailure => Category::AuthFailure,
        MatchKind::RateLimit => {
            if USAGE_LIMIT_TEXT.is_match(&text_match.snippet) {
                Category::RateLimit(RateLimitClass::UsageLimitExhausted)
            } else {
                Category::RateLimit(RateLimitClass::Transient)
            }
        }
    };
    Some(Classification {
        category,
        reset_after_seconds: parse_reset_after_seconds(&text_match.snippet),
        reset_source: parse_reset_after_seconds(&text_match.snippet)
            .map(|_| ResetSource::ParsedText),
        snippet: text_match.snippet,
    })
}

#[allow(dead_code)]
fn classify_structured(events: &EventSummary) -> Option<Classification> {
    let turn_error = events.turn_error.as_ref()?;
    let snippet = if turn_error.message.is_empty() {
        "structured error event".to_owned()
    } else {
        turn_error.message.clone()
    };
    let (reset_after_seconds, reset_source) =
        select_reset_after_seconds(turn_error, events.last_rate_limits.as_ref())
            .map_or((None, None), |(seconds, source)| {
                (Some(seconds), Some(source))
            });

    if matches!(
        turn_error.error_code.as_deref(),
        Some("usage_limit_reached" | "usage_limit_exceeded")
    ) {
        return Some(Classification {
            category: Category::RateLimit(RateLimitClass::UsageLimitExhausted),
            reset_after_seconds,
            reset_source,
            snippet,
        });
    }

    if turn_error.error_code.as_deref() == Some("context_window_exceeded") {
        return Some(Classification {
            category: Category::ContextWindowExceeded,
            reset_after_seconds,
            reset_source,
            snippet,
        });
    }

    // Checked before the bare rate-limit signal so a future credit message
    // that also happens to contain rate-limit language keeps the more specific
    // classification. The observed shape carries no reset information; the
    // handlers derive the cooldown from the account's own usage data instead
    // (see `retry::credit_cooldown`).
    if is_credit_exhausted(turn_error) {
        return Some(Classification {
            category: Category::CreditExhausted,
            reset_after_seconds,
            reset_source,
            snippet,
        });
    }

    if is_rate_limit_signal(turn_error, events.last_rate_limits.as_ref()) {
        // A bare rate-limit signal (no explicit usage-limit code) is treated as
        // `Transient` — back off the same account — UNLESS the snapshot proves
        // the window is spent (`used_percent >= 99`), in which case escalate to
        // `UsageLimitExhausted` and rotate. A missing snapshot or one with
        // headroom stays `Transient`: that is the production-bug case (a bare
        // 429 fired at healthy quota) the wrapper must back off rather than
        // burning the whole pool into flat cooldowns.
        let class = if snapshot_window_exhausted(events.last_rate_limits.as_ref()) {
            RateLimitClass::UsageLimitExhausted
        } else {
            RateLimitClass::Transient
        };
        return Some(Classification {
            category: Category::RateLimit(class),
            reset_after_seconds,
            reset_source,
            snippet,
        });
    }

    if is_auth_failure(turn_error) {
        // A structured `401` (or known auth code/message) must route to the
        // refresh-then-rotate AuthFailure path, not the unhandled fallback.
        // Without this branch a `turn.failed` carrying `http_status_code: 401`
        // whose text representation lacks a recognizable `401`/`unauthorized`
        // token would fall through to `Unclassified` and skip the token refresh.
        return Some(Classification {
            category: Category::AuthFailure,
            reset_after_seconds,
            reset_source,
            snippet,
        });
    }

    if is_server_error(turn_error) {
        return Some(Classification {
            category: Category::ServerError,
            reset_after_seconds,
            reset_source,
            snippet,
        });
    }

    if is_no_rollout_found(turn_error) {
        return Some(Classification {
            category: Category::NoRolloutFound,
            reset_after_seconds,
            reset_source,
            snippet,
        });
    }

    Some(Classification {
        category: Category::Unclassified,
        reset_after_seconds,
        reset_source,
        snippet,
    })
}

/// Credit exhaustion, observed live on codex 0.135.0 (2026-06-03) as an
/// `error` + `turn.failed` pair carrying ONLY a message — "Your workspace is
/// out of credits. Add credits to continue." — with no `error_code` and no
/// `http_status_code` (see docs/upstream-codex.md §F9). Upstream's
/// `RateLimitReachedType` (`codex-rs/protocol/src/protocol.rs`) has both
/// `WorkspaceOwnerCreditsDepleted` and `WorkspaceMemberCreditsDepleted`
/// variants, so the plain "out of credits" substring is matched deliberately
/// to cover both phrasings.
fn is_credit_exhausted(turn_error: &TurnError) -> bool {
    turn_error
        .message
        .to_ascii_lowercase()
        .contains("out of credits")
}

fn is_auth_failure(turn_error: &TurnError) -> bool {
    if turn_error.http_status == Some(401) {
        return true;
    }
    let lower = turn_error.message.to_ascii_lowercase();
    lower.contains("unauthorized")
        || lower.contains("invalid auth")
        || lower.contains("invalid_grant")
        || lower.contains("token exchange")
        || lower.contains("insufficient permissions")
}

#[allow(dead_code)]
fn select_reset_after_seconds(
    turn_error: &TurnError,
    snapshot: Option<&RateLimitSnapshot>,
) -> Option<(u64, ResetSource)> {
    turn_error
        .retry_after_seconds
        .map(|seconds| (seconds, ResetSource::RetryAfter))
        .or_else(|| {
            relevant_window(snapshot)
                .and_then(|window| window.resets_in_seconds)
                .map(|seconds| (seconds, ResetSource::ServerReset))
        })
        .or_else(|| {
            parse_reset_after_seconds(&turn_error.message)
                .map(|seconds| (seconds, ResetSource::ParsedText))
        })
}

#[allow(dead_code)]
fn relevant_window(snapshot: Option<&RateLimitSnapshot>) -> Option<&RateLimitWindow> {
    let snapshot = snapshot?;
    match snapshot.rate_limit_reached_type.as_deref() {
        Some(kind) if kind.contains("secondary") => snapshot.secondary.as_ref(),
        Some(kind) if kind.contains("primary") => snapshot.primary.as_ref(),
        _ => snapshot.primary.as_ref().or(snapshot.secondary.as_ref()),
    }
}

/// True when the relevant rate-limit window proves the quota is spent
/// (`used_percent >= 99`). Used to escalate a bare rate-limit signal (one with
/// no explicit `usage_limit_*` code) from `Transient` to `UsageLimitExhausted`.
/// An absent snapshot, or one that still reports headroom, returns `false` so
/// the signal stays `Transient`.
#[allow(dead_code)]
fn snapshot_window_exhausted(snapshot: Option<&RateLimitSnapshot>) -> bool {
    relevant_window(snapshot)
        .and_then(|window| window.used_percent)
        .is_some_and(|used_percent| used_percent >= 99.0)
}

#[allow(dead_code)]
fn is_rate_limit_signal(turn_error: &TurnError, snapshot: Option<&RateLimitSnapshot>) -> bool {
    if turn_error.http_status == Some(429) {
        return true;
    }
    if contains_rate_limit_language(&turn_error.message) {
        return true;
    }
    snapshot
        .and_then(|value| value.rate_limit_reached_type.as_deref())
        .is_some_and(|value| !value.trim().is_empty())
}

fn is_server_error(turn_error: &TurnError) -> bool {
    if matches!(
        turn_error.http_status,
        Some(status) if (500..=599).contains(&status)
    ) {
        return true;
    }
    let lower = turn_error.message.to_ascii_lowercase();
    lower.contains("stream error")
        || lower.contains("server error")
        || lower.contains("internal server error")
        || lower.contains("upstream error")
}

fn is_no_rollout_found(turn_error: &TurnError) -> bool {
    contains_no_rollout_language(&turn_error.message)
        || turn_error
            .error_code
            .as_deref()
            .is_some_and(contains_no_rollout_language)
}

#[allow(dead_code)]
fn contains_rate_limit_language(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("rate limit")
        || lower.contains("rate-limit")
        || lower.contains("too many requests")
        || lower.contains("quota exceeded")
        || lower.contains("slow down")
        || lower.contains("429")
}

#[allow(dead_code)]
fn parse_reset_after_seconds(snippet: &str) -> Option<u64> {
    let captures = TRY_AGAIN_IN.captures(snippet)?;
    let value = captures.get(1)?.as_str().parse::<f64>().ok()?;
    let unit = captures.get(2)?.as_str().to_ascii_lowercase();
    let seconds = if unit == "ms" { value / 1000.0 } else { value };
    f64_floor_to_u64_saturating(seconds)
}

#[allow(dead_code)]
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

#[allow(dead_code)]
fn scan_usage_limit_text(buf: &[u8]) -> Option<String> {
    for line in buf.split(|byte| *byte == b'\n') {
        let line = String::from_utf8_lossy(line);
        if USAGE_LIMIT_TEXT.is_match(&line) {
            return Some(truncate_for_debug(&line, 256));
        }
    }
    None
}

fn scan_no_rollout_text(buf: &[u8]) -> Option<String> {
    for line in buf.split(|byte| *byte == b'\n') {
        let line = String::from_utf8_lossy(line);
        if contains_no_rollout_language(&line) {
            return Some(truncate_for_debug(&line, 256));
        }
    }
    None
}

fn contains_no_rollout_language(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("no rollout found")
        || lower.contains("thread not found")
        // `-32600` is the generic JSON-RPC "Invalid Request" code; only treat
        // it as a no-rollout signal alongside resume/thread language so
        // unrelated invalid-request errors keep their own classification.
        || (lower.contains("-32600")
            && (lower.contains("rollout") || lower.contains("thread")))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{
        Category, Match, MatchKind, RateLimitClass, ResetSource, classify, pattern_name,
        pick_priority, scan,
    };
    use crate::services::account::codex_events::{
        EventSummary, RateLimitSnapshot, RateLimitWindow, TurnError, scan_events,
    };

    #[test]
    fn matches_http_429_too_many_requests() {
        assert_eq!(
            scan(b"HTTP 429 Too Many Requests\n"),
            Some(Match {
                kind: MatchKind::RateLimit,
                line_no: 1,
                snippet: "HTTP 429 Too Many Requests".to_owned(),
                pattern_index: 2,
            })
        );
    }

    #[test]
    fn matches_rate_limit_phrase() {
        assert_eq!(
            scan(b"Error: rate limit exceeded\n"),
            Some(Match {
                kind: MatchKind::RateLimit,
                line_no: 1,
                snippet: "Error: rate limit exceeded".to_owned(),
                pattern_index: 0,
            })
        );
    }

    #[test]
    fn matches_rate_limit_with_hyphen() {
        assert_eq!(
            scan(b"Error: rate-limit exceeded\n"),
            Some(Match {
                kind: MatchKind::RateLimit,
                line_no: 1,
                snippet: "Error: rate-limit exceeded".to_owned(),
                pattern_index: 0,
            })
        );
    }

    #[test]
    fn matches_quota_exceeded() {
        assert_eq!(
            scan(b"quota exceeded; retry after 300 s\n"),
            Some(Match {
                kind: MatchKind::RateLimit,
                line_no: 1,
                snippet: "quota exceeded; retry after 300 s".to_owned(),
                pattern_index: 1,
            })
        );
    }

    #[test]
    fn matches_slow_down_with_space() {
        assert_eq!(
            scan(b"Server says slow down\n"),
            Some(Match {
                kind: MatchKind::RateLimit,
                line_no: 1,
                snippet: "Server says slow down".to_owned(),
                pattern_index: 5,
            })
        );
    }

    #[test]
    fn matches_slow_down_with_hyphen() {
        assert_eq!(
            scan(b"Server says slow-down\n"),
            Some(Match {
                kind: MatchKind::RateLimit,
                line_no: 1,
                snippet: "Server says slow-down".to_owned(),
                pattern_index: 5,
            })
        );
    }

    #[test]
    fn matches_exceeded_rate_phrase() {
        assert_eq!(
            scan(b"Error: exceeded rate quota\n"),
            Some(Match {
                kind: MatchKind::RateLimit,
                line_no: 1,
                snippet: "Error: exceeded rate quota".to_owned(),
                pattern_index: 4,
            })
        );
    }

    #[test]
    fn matches_rate_limit_json_key_because_caam_wildcard_allows_it() {
        assert_eq!(
            scan(b"rate_limit: { ... }\n"),
            Some(Match {
                kind: MatchKind::RateLimit,
                line_no: 1,
                snippet: "rate_limit: { ... }".to_owned(),
                pattern_index: 0,
            })
        );
    }

    #[test]
    fn rejects_embedded_429_digits() {
        assert_eq!(scan(b"Got 9429 errors today\n"), None);
    }

    #[test]
    fn rejects_spelled_out_429() {
        assert_eq!(scan(b"four hundred twenty nine\n"), None);
    }

    #[test]
    fn rejects_claude_capacity_phrase() {
        assert_eq!(scan(b"nominal capacity reached\n"), None);
    }

    #[test]
    fn matches_auth_401() {
        assert_eq!(
            scan(b"HTTP 401 Unauthorized\n"),
            Some(Match {
                kind: MatchKind::AuthFailure,
                line_no: 1,
                snippet: "HTTP 401 Unauthorized".to_owned(),
                pattern_index: 0,
            })
        );
    }

    #[test]
    fn matches_auth_unauthorized() {
        assert_eq!(
            scan(b"Error: Unauthorized access\n"),
            Some(Match {
                kind: MatchKind::AuthFailure,
                line_no: 1,
                snippet: "Error: Unauthorized access".to_owned(),
                pattern_index: 1,
            })
        );
    }

    #[test]
    fn matches_auth_invalid_auth() {
        assert_eq!(
            scan(b"invalid auth token\n"),
            Some(Match {
                kind: MatchKind::AuthFailure,
                line_no: 1,
                snippet: "invalid auth token".to_owned(),
                pattern_index: 2,
            })
        );
    }

    #[test]
    fn matches_auth_no_auth_credentials() {
        assert_eq!(
            scan(b"no auth credentials found\n"),
            Some(Match {
                kind: MatchKind::AuthFailure,
                line_no: 1,
                snippet: "no auth credentials found".to_owned(),
                pattern_index: 3,
            })
        );
    }

    #[test]
    fn matches_auth_invalid_grant() {
        assert_eq!(
            scan(b"error: invalid_grant\n"),
            Some(Match {
                kind: MatchKind::AuthFailure,
                line_no: 1,
                snippet: "error: invalid_grant".to_owned(),
                pattern_index: 4,
            })
        );
    }

    #[test]
    fn matches_auth_token_exchange_error() {
        assert_eq!(
            scan(b"token exchange error\n"),
            Some(Match {
                kind: MatchKind::AuthFailure,
                line_no: 1,
                snippet: "token exchange error".to_owned(),
                pattern_index: 5,
            })
        );
    }

    #[test]
    fn matches_auth_insufficient_permissions() {
        assert_eq!(
            scan(b"insufficient permissions for this action\n"),
            Some(Match {
                kind: MatchKind::AuthFailure,
                line_no: 1,
                snippet: "insufficient permissions for this action".to_owned(),
                pattern_index: 6,
            })
        );
    }

    #[test]
    fn rejects_embedded_401_digits() {
        assert_eq!(scan(b"Got 9401 errors today\n"), None);
    }

    #[test]
    fn rate_limit_wins_when_both_match() {
        assert_eq!(
            scan(b"HTTP 429 Unauthorized\n"),
            Some(Match {
                kind: MatchKind::RateLimit,
                line_no: 1,
                snippet: "HTTP 429 Unauthorized".to_owned(),
                pattern_index: 2,
            })
        );
    }

    #[test]
    fn pattern_name_uses_kind_specific_arrays() {
        assert_eq!(
            pattern_name(&Match {
                kind: MatchKind::RateLimit,
                line_no: 1,
                snippet: String::new(),
                pattern_index: 2,
            }),
            "429"
        );
        assert_eq!(
            pattern_name(&Match {
                kind: MatchKind::AuthFailure,
                line_no: 1,
                snippet: String::new(),
                pattern_index: 0,
            }),
            "401"
        );
    }

    fn rate_limit_match() -> Match {
        Match {
            kind: MatchKind::RateLimit,
            line_no: 1,
            snippet: "HTTP 429 Too Many Requests".to_owned(),
            pattern_index: 2,
        }
    }

    fn auth_failure_match() -> Match {
        Match {
            kind: MatchKind::AuthFailure,
            line_no: 1,
            snippet: "HTTP 401 Unauthorized".to_owned(),
            pattern_index: 0,
        }
    }

    #[test]
    fn pick_priority_rate_limit_err_wins_over_auth_out() {
        let result = pick_priority(Some(rate_limit_match()), Some(auth_failure_match()));
        assert_eq!(result.unwrap().kind, MatchKind::RateLimit);
    }

    #[test]
    fn pick_priority_rate_limit_out_wins_over_auth_err() {
        let result = pick_priority(Some(auth_failure_match()), Some(rate_limit_match()));
        assert_eq!(result.unwrap().kind, MatchKind::RateLimit);
    }

    #[test]
    fn pick_priority_both_auth_prefers_err_stream() {
        let mut stderr_auth = auth_failure_match();
        stderr_auth.snippet = "stderr-401".to_owned();
        let mut stdout_auth = auth_failure_match();
        stdout_auth.snippet = "stdout-401".to_owned();
        let result = pick_priority(Some(stderr_auth), Some(stdout_auth));
        assert_eq!(result.as_ref().unwrap().snippet, "stderr-401");
    }

    #[test]
    fn pick_priority_err_stream_only() {
        let result = pick_priority(Some(auth_failure_match()), None);
        assert!(result.is_some());
    }

    #[test]
    fn pick_priority_out_stream_only() {
        let result = pick_priority(None, Some(rate_limit_match()));
        assert!(result.is_some());
    }

    #[test]
    fn pick_priority_none() {
        assert!(pick_priority(None, None).is_none());
    }

    #[test]
    fn classify_structured_usage_limit_jsonl() {
        let token_count = serde_json::json!({
            "type": "token_count",
            "rate_limits": {
                "primary": {"used_percent": 100.0, "resets_in_seconds": 88},
                "rate_limit_reached_type": "primary",
            },
        })
        .to_string();
        let turn_failed = serde_json::json!({
            "type": "turn.failed",
            "message": "usage limit hit",
            "error": {
                "error_code": "usage_limit_reached",
                "retry_after": 44,
                "http_status_code": 429,
            },
        })
        .to_string();
        let stdout = format!("{token_count}\n{turn_failed}\n");
        let events = scan_events(stdout.as_bytes());

        let result = classify(&events, stdout.as_bytes(), b"").unwrap();

        assert_eq!(
            result.category,
            Category::RateLimit(RateLimitClass::UsageLimitExhausted)
        );
        assert_eq!(result.reset_after_seconds, Some(44));
        assert_eq!(result.reset_source, Some(ResetSource::RetryAfter));
    }

    #[test]
    fn classify_structured_reset_uses_snapshot_when_retry_after_missing() {
        let events = EventSummary {
            last_rate_limits: Some(RateLimitSnapshot {
                primary: Some(RateLimitWindow {
                    resets_in_seconds: Some(21),
                    used_percent: Some(100.0),
                    ..RateLimitWindow::default()
                }),
                rate_limit_reached_type: Some("primary".to_owned()),
                ..RateLimitSnapshot::default()
            }),
            turn_error: Some(TurnError {
                message: "usage_limit_exceeded".to_owned(),
                error_code: Some("usage_limit_exceeded".to_owned()),
                retry_after_seconds: None,
                http_status: Some(429),
            }),
        };

        let result = classify(&events, b"", b"").unwrap();

        assert_eq!(result.reset_after_seconds, Some(21));
        assert_eq!(result.reset_source, Some(ResetSource::ServerReset));
    }

    #[test]
    fn classify_structured_no_rollout_found() {
        let events = EventSummary {
            turn_error: Some(TurnError {
                message: "thread/resume failed: no rollout found for thread id abc (code -32600)"
                    .to_owned(),
                error_code: Some("invalid_request".to_owned()),
                retry_after_seconds: None,
                http_status: Some(400),
            }),
            ..EventSummary::default()
        };

        let result = classify(&events, b"", b"").unwrap();
        assert_eq!(result.category, Category::NoRolloutFound);
    }

    #[test]
    fn classify_text_thread_not_found_as_no_rollout_found() {
        let result = classify(&EventSummary::default(), b"", b"thread not found\n").unwrap();
        assert_eq!(result.category, Category::NoRolloutFound);
    }

    #[test]
    fn classify_text_code_minus_32600_with_thread_language_as_no_rollout_found() {
        let result = classify(
            &EventSummary::default(),
            b"",
            b"thread/resume request failed (code -32600)\n",
        )
        .unwrap();
        assert_eq!(result.category, Category::NoRolloutFound);
    }

    #[test]
    fn classify_text_bare_minus_32600_is_not_no_rollout_found() {
        let result = classify(
            &EventSummary::default(),
            b"",
            b"request failed (code -32600)\n",
        );
        assert!(
            result.is_none_or(|c| c.category != Category::NoRolloutFound),
            "bare -32600 without rollout/thread language must not classify as NoRolloutFound"
        );
    }

    #[test]
    fn classify_structured_429_without_usage_limit_code_is_transient() {
        let events = EventSummary {
            last_rate_limits: None,
            turn_error: Some(TurnError {
                message: "HTTP 429 Too Many Requests".to_owned(),
                error_code: None,
                retry_after_seconds: Some(9),
                http_status: Some(429),
            }),
        };

        let result = classify(&events, b"", b"").unwrap();

        assert_eq!(
            result.category,
            Category::RateLimit(RateLimitClass::Transient)
        );
        assert_eq!(result.reset_after_seconds, Some(9));
        assert_eq!(result.reset_source, Some(ResetSource::RetryAfter));
    }

    #[test]
    fn classify_structured_headroom_biases_429_to_transient() {
        let events = EventSummary {
            last_rate_limits: Some(RateLimitSnapshot {
                primary: Some(RateLimitWindow {
                    used_percent: Some(42.0),
                    resets_in_seconds: Some(12),
                    ..RateLimitWindow::default()
                }),
                rate_limit_reached_type: Some("primary".to_owned()),
                ..RateLimitSnapshot::default()
            }),
            turn_error: Some(TurnError {
                message: "rate limit reached".to_owned(),
                error_code: None,
                retry_after_seconds: None,
                http_status: Some(429),
            }),
        };

        let result = classify(&events, b"", b"").unwrap();

        assert_eq!(
            result.category,
            Category::RateLimit(RateLimitClass::Transient)
        );
        assert_eq!(result.reset_after_seconds, Some(12));
        assert_eq!(result.reset_source, Some(ResetSource::ServerReset));
    }

    #[test]
    fn classify_structured_429_with_exhausted_snapshot_is_usage_limit() {
        // A bare 429 (no explicit usage-limit code) whose snapshot proves the
        // window is spent must escalate to UsageLimitExhausted (rotate), not
        // Transient. This pins that `snapshot_window_exhausted` is load-bearing.
        let events = EventSummary {
            last_rate_limits: Some(RateLimitSnapshot {
                primary: Some(RateLimitWindow {
                    used_percent: Some(99.5),
                    resets_in_seconds: Some(120),
                    ..RateLimitWindow::default()
                }),
                rate_limit_reached_type: Some("primary".to_owned()),
                ..RateLimitSnapshot::default()
            }),
            turn_error: Some(TurnError {
                message: "rate limit reached".to_owned(),
                error_code: None,
                retry_after_seconds: None,
                http_status: Some(429),
            }),
        };

        let result = classify(&events, b"", b"").unwrap();

        assert_eq!(
            result.category,
            Category::RateLimit(RateLimitClass::UsageLimitExhausted)
        );
        assert_eq!(result.reset_after_seconds, Some(120));
        assert_eq!(result.reset_source, Some(ResetSource::ServerReset));
    }

    #[test]
    fn classify_structured_out_of_credits_is_credit_exhausted() {
        // The live shape (docs/upstream-codex.md §F9): `turn.failed` carrying
        // only a message — no error_code, no http_status, no retry_after.
        let events = EventSummary {
            last_rate_limits: None,
            turn_error: Some(TurnError {
                message: "Your workspace is out of credits. Add credits to continue.".to_owned(),
                error_code: None,
                retry_after_seconds: None,
                http_status: None,
            }),
        };

        let result = classify(&events, b"", b"").unwrap();

        assert_eq!(result.category, Category::CreditExhausted);
        assert_eq!(result.reset_after_seconds, None);
        assert_eq!(result.reset_source, None);
        assert_eq!(
            result.snippet,
            "Your workspace is out of credits. Add credits to continue."
        );
    }

    #[test]
    fn classify_out_of_credits_is_case_insensitive() {
        let events = EventSummary {
            last_rate_limits: None,
            turn_error: Some(TurnError {
                message: "OUT OF CREDITS".to_owned(),
                error_code: None,
                retry_after_seconds: None,
                http_status: None,
            }),
        };

        let result = classify(&events, b"", b"").unwrap();
        assert_eq!(result.category, Category::CreditExhausted);
    }

    #[test]
    fn classify_unrelated_credits_mention_is_not_credit_exhausted() {
        // Only the "out of credits" phrase classifies; a stray "credits"
        // mention must not.
        let events = EventSummary {
            last_rate_limits: None,
            turn_error: Some(TurnError {
                message: "credits to the team for this failure".to_owned(),
                error_code: None,
                retry_after_seconds: None,
                http_status: None,
            }),
        };

        let result = classify(&events, b"", b"").unwrap();
        assert_eq!(result.category, Category::Unclassified);
    }

    #[test]
    fn classify_structured_context_window_exceeded() {
        let events = EventSummary {
            last_rate_limits: None,
            turn_error: Some(TurnError {
                message: "context_window_exceeded".to_owned(),
                error_code: Some("context_window_exceeded".to_owned()),
                retry_after_seconds: None,
                http_status: None,
            }),
        };

        let result = classify(&events, b"", b"").unwrap();

        assert_eq!(result.category, Category::ContextWindowExceeded);
        assert_eq!(result.reset_source, None);
    }

    #[test]
    fn classify_structured_server_error() {
        let events = EventSummary {
            last_rate_limits: None,
            turn_error: Some(TurnError {
                message: "stream error from upstream".to_owned(),
                error_code: None,
                retry_after_seconds: None,
                http_status: Some(503),
            }),
        };

        let result = classify(&events, b"", b"").unwrap();

        assert_eq!(result.category, Category::ServerError);
        assert_eq!(result.reset_source, None);
    }

    #[test]
    fn classify_structured_401_without_text_token_is_auth_failure() {
        // A structured `turn.failed` carrying `http_status_code: 401` whose
        // message lacks a recognizable `401`/`unauthorized` token must still
        // route to AuthFailure (refresh-then-rotate), not the unhandled path.
        let events = EventSummary {
            last_rate_limits: None,
            turn_error: Some(TurnError {
                message: "authentication failed".to_owned(),
                error_code: None,
                retry_after_seconds: None,
                http_status: Some(401),
            }),
        };

        let result = classify(&events, b"", b"").unwrap();

        assert_eq!(result.category, Category::AuthFailure);
        assert_eq!(result.snippet, "authentication failed");
    }

    #[test]
    fn classify_text_fallback_auth_and_generic_429() {
        let auth = classify(&EventSummary::default(), b"", b"HTTP 401 Unauthorized\n").unwrap();
        assert_eq!(auth.category, Category::AuthFailure);
        assert_eq!(auth.reset_source, None);

        let rate = classify(
            &EventSummary::default(),
            b"HTTP 429 Too Many Requests\n",
            b"",
        )
        .unwrap();
        assert_eq!(
            rate.category,
            Category::RateLimit(RateLimitClass::Transient)
        );
        assert_eq!(rate.reset_source, None);
    }

    #[test]
    fn classify_text_usage_limit_phrase_is_exhausted() {
        let result = classify(
            &EventSummary::default(),
            b"You hit your usage limit. Try again in 12s\n",
            b"",
        )
        .unwrap();

        assert_eq!(
            result.category,
            Category::RateLimit(RateLimitClass::UsageLimitExhausted)
        );
        assert_eq!(result.reset_after_seconds, Some(12));
        assert_eq!(result.reset_source, Some(ResetSource::ParsedText));
    }

    #[test]
    fn classify_text_parses_ms_duration() {
        let result = classify(
            &EventSummary::default(),
            b"rate limit exceeded; try again in 12000ms\n",
            b"",
        )
        .unwrap();

        assert_eq!(
            result.category,
            Category::RateLimit(RateLimitClass::Transient)
        );
        assert_eq!(result.reset_after_seconds, Some(12));
        assert_eq!(result.reset_source, Some(ResetSource::ParsedText));
    }

    #[test]
    fn classify_structured_unknown_error_with_no_text_signal_is_unclassified() {
        let events = EventSummary {
            last_rate_limits: Some(RateLimitSnapshot::default()),
            turn_error: Some(TurnError {
                message: "weird failure".to_owned(),
                error_code: None,
                retry_after_seconds: None,
                http_status: None,
            }),
        };

        // No usable text signal in stdout/stderr: the structured `Unclassified`
        // (carrying its snippet) is preserved for Round 3's general handler.
        let result = classify(&events, b"all quiet here\n", b"").unwrap();

        assert_eq!(result.category, Category::Unclassified);
        assert_eq!(result.reset_source, None);
        assert_eq!(result.snippet, "weird failure");
    }

    #[test]
    fn classify_unclassified_structured_defers_to_stronger_text_signal() {
        // A sparse `turn.failed` that classifies as `Unclassified` must NOT
        // suppress a stronger legacy text signal — a 401/429 in stderr/stdout
        // still wins over the structured non-signal.
        let events = EventSummary {
            last_rate_limits: Some(RateLimitSnapshot::default()),
            turn_error: Some(TurnError {
                message: "weird failure".to_owned(),
                error_code: None,
                retry_after_seconds: None,
                http_status: None,
            }),
        };

        let auth = classify(&events, b"", b"HTTP 401 Unauthorized\n").unwrap();
        assert_eq!(auth.category, Category::AuthFailure);

        let rate = classify(&events, b"HTTP 429 Too Many Requests\n", b"").unwrap();
        assert_eq!(
            rate.category,
            Category::RateLimit(RateLimitClass::Transient)
        );
    }

    #[test]
    fn classify_structured_snapshot_without_turn_error_returns_none() {
        let events = EventSummary {
            last_rate_limits: Some(RateLimitSnapshot {
                primary: Some(RateLimitWindow {
                    used_percent: Some(45.0),
                    resets_in_seconds: Some(18),
                    ..RateLimitWindow::default()
                }),
                rate_limit_reached_type: Some("primary".to_owned()),
                ..RateLimitSnapshot::default()
            }),
            turn_error: None,
        };

        assert!(classify(&events, b"", b"").is_none());
    }

    #[test]
    fn classify_snapshot_without_structured_error_can_fall_back_to_text() {
        let events = EventSummary {
            last_rate_limits: Some(RateLimitSnapshot::default()),
            turn_error: None,
        };

        let result = classify(
            &events,
            b"You hit your usage limit. Try again in 12s\n",
            b"",
        )
        .unwrap();

        assert_eq!(
            result.category,
            Category::RateLimit(RateLimitClass::UsageLimitExhausted)
        );
        assert_eq!(result.reset_after_seconds, Some(12));
        assert_eq!(result.reset_source, Some(ResetSource::ParsedText));
    }

    #[test]
    fn classify_no_signal_returns_none() {
        assert!(classify(&EventSummary::default(), b"all good\n", b"").is_none());
    }
}
