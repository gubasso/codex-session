//! Codex CLI v0.134+ compatibility checks.
//!
//! The wrapper's emitted `$CODEX_HOME/` tree targets the v0.134+ input shape
//! (sibling `<name>.config.toml` files, no `[profiles.*]` table). If the
//! installed `codex` binary is older, the child rejects our emitted file with
//! a misleading legacy-form error. This module gates that case up front.

use std::sync::OnceLock;

use semver::Version;

pub(crate) const REQUIRED_CODEX_VERSION: &str = "0.134.0";

/// Lowest possible pre-release of `REQUIRED_CODEX_VERSION`.
///
/// This is the comparison floor so `0.134.0-alpha.1 >= floor` evaluates true
/// while `0.133.99 >= floor` evaluates false. `semver::Version::cmp_precedence`
/// follows `SemVer` 2.0.0 section 11 ordering, where numeric pre-release
/// identifiers sort before non-numeric identifiers.
#[allow(clippy::expect_used)]
pub(crate) fn required_floor() -> &'static Version {
    static FLOOR: OnceLock<Version> = OnceLock::new();
    FLOOR.get_or_init(|| Version::parse("0.134.0-0").expect("hardcoded floor parses"))
}

/// Parsed result of a child `codex --version` probe.
#[derive(Debug, Clone)]
pub(crate) enum VersionCheck {
    /// The child version satisfies the required floor.
    Ok(Version),
    /// The child version is older than the required floor.
    TooOld(Version),
    /// The version output could not be parsed.
    Unparseable(String),
}

/// Classify a `codex --version` line.
///
/// Tolerates a leading `codex` or `codex-cli` token and surrounding whitespace.
pub(crate) fn classify(raw: &str) -> VersionCheck {
    let trimmed = raw.trim();
    let body = strip_codex_prefix(trimmed);
    Version::parse(body).map_or_else(
        |_| VersionCheck::Unparseable(raw.to_owned()),
        |version| {
            if version.cmp_precedence(required_floor()).is_lt() {
                VersionCheck::TooOld(version)
            } else {
                VersionCheck::Ok(version)
            }
        },
    )
}

fn strip_codex_prefix(trimmed: &str) -> &str {
    let mut parts = trimmed.split_whitespace();
    let Some(first) = parts.next() else {
        return trimmed;
    };
    if first != "codex" && first != "codex-cli" {
        return trimmed;
    }
    trimmed[first.len()..].trim()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ok_exact_floor() {
        assert!(matches!(classify("codex 0.134.0\n"), VersionCheck::Ok(_)));
    }

    #[test]
    fn ok_codex_cli_prefix() {
        assert!(matches!(
            classify("codex-cli 0.134.0\n"),
            VersionCheck::Ok(_)
        ));
    }

    #[test]
    fn ok_whitespace_tolerated() {
        assert!(matches!(
            classify("  codex 0.134.0  \n"),
            VersionCheck::Ok(_)
        ));
    }

    #[test]
    fn ok_prerelease_rc1() {
        assert!(matches!(classify("codex 0.134.0-rc1"), VersionCheck::Ok(_)));
    }

    #[test]
    fn ok_prerelease_alpha() {
        assert!(matches!(
            classify("codex 0.134.0-alpha.1"),
            VersionCheck::Ok(_)
        ));
    }

    #[test]
    fn ok_higher_minor() {
        assert!(matches!(classify("codex 0.135.0"), VersionCheck::Ok(_)));
    }

    #[test]
    fn ok_one_zero_zero() {
        assert!(matches!(classify("codex 1.0.0"), VersionCheck::Ok(_)));
    }

    #[test]
    fn too_old_just_below() {
        assert!(matches!(
            classify("codex 0.133.99"),
            VersionCheck::TooOld(_)
        ));
    }

    #[test]
    fn too_old_far_below() {
        assert!(matches!(classify("codex 0.0.1"), VersionCheck::TooOld(_)));
    }

    #[test]
    fn unparseable_garbage() {
        assert!(matches!(
            classify("garbage output"),
            VersionCheck::Unparseable(_)
        ));
    }

    #[test]
    #[allow(clippy::panic)]
    fn unparseable_sentinel() {
        match classify("<unavailable>") {
            VersionCheck::Unparseable(raw) => assert_eq!(raw, "<unavailable>"),
            other => panic!("expected Unparseable, got {other:?}"),
        }
    }
}
