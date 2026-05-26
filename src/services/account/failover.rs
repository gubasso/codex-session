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

use regex::RegexSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MatchKind {
    RateLimit,
    AuthFailure,
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

pub(crate) const fn pattern_name(matched: &Match) -> &'static str {
    match matched.kind {
        MatchKind::RateLimit => RATE_LIMIT_PATTERN_NAMES[matched.pattern_index],
        MatchKind::AuthFailure => AUTH_FAILURE_PATTERN_NAMES[matched.pattern_index],
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{Match, MatchKind, pattern_name, pick_priority, scan};

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
}
