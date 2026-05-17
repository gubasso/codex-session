//! Line-oriented merge helpers.

use std::collections::HashSet;

/// Extract target-only top-level sections using bash-compatible line rules.
pub(crate) fn extract_local_sections(base: &str, target: &str) -> String {
    let base_headers: HashSet<&str> = base.lines().filter(|line| line.starts_with('[')).collect();
    let mut out = String::new();
    let mut local = false;

    for line in target.lines() {
        if line.starts_with('[') {
            local = !base_headers.contains(line);
        }
        if local {
            out.push_str(line);
            out.push('\n');
        }
    }

    out
}

/// Merge base contents with any preserved local sections.
pub(crate) fn merge_contents(base: &str, local_sections: &str) -> String {
    let mut out = String::with_capacity(base.len() + local_sections.len() + 64);
    out.push_str(base);
    if !local_sections.is_empty() {
        out.push_str("\n# === Machine-local (preserved by codex-session) ===\n");
        out.push_str(local_sections);
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    const BASE: &str = include_str!("../../tests/fixtures/base.toml");
    const TARGET: &str = include_str!("../../tests/fixtures/target-with-local.toml");
    const EXPECTED: &str = include_str!("../../tests/fixtures/expected-merged.toml");

    #[test]
    fn extract_local_matches_bash_awk() {
        let local = super::extract_local_sections(BASE, TARGET);
        let (_, after) = EXPECTED
            .split_once("# === Machine-local (preserved by codex-session) ===\n")
            .unwrap();
        assert_eq!(local, after);
    }

    #[test]
    fn merge_matches_expected_fixture() {
        let local = super::extract_local_sections(BASE, TARGET);
        let merged = super::merge_contents(BASE, &local);
        assert_eq!(merged, EXPECTED);
    }
}
