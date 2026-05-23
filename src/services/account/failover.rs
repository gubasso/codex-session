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

const PATTERNS_RAW: [&str; 6] = [
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
static PATTERNS: LazyLock<RegexSet> =
    LazyLock::new(|| RegexSet::new(PATTERNS_RAW).expect("static regex set must compile"));

pub(crate) const PATTERN_NAMES: [&str; 6] = [
    "rate-limit",
    "quota-exceeded",
    "429",
    "too-many-requests",
    "exceeded-rate",
    "slow-down",
];

/// `pattern_index` is the lowest matching index in `PATTERNS_RAW`, because
/// `RegexSet::matches().into_iter().next()` yields the lowest matching index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Match {
    pub line_no: usize,
    pub snippet: String,
    pub pattern_index: usize,
}

pub(crate) fn scan(buf: &[u8]) -> Option<Match> {
    // from_utf8_lossy keeps us correct on non-UTF-8 captures (e.g. ANSI
    // escape sequences with raw bytes); do not switch to a strict
    // str::from_utf8 over the whole buffer.
    for (idx, line) in buf.split(|byte| *byte == b'\n').enumerate() {
        let line = String::from_utf8_lossy(line);
        let matches = PATTERNS.matches(&line);
        if let Some(pattern_index) = matches.into_iter().next() {
            return Some(Match {
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

#[cfg(test)]
mod tests {
    use super::{Match, scan};

    #[test]
    fn matches_http_429_too_many_requests() {
        assert_eq!(
            scan(b"HTTP 429 Too Many Requests\n"),
            Some(Match {
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
}
