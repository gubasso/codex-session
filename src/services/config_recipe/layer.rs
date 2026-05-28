//! Config-recipe settings layer parsing and merge logic.
//!
//! What this is: TOML table parsing, deep merge, and `[env]` extraction.
//! What this is not: manifest parsing or filesystem layout decisions.

#![allow(clippy::result_large_err)]

use std::collections::BTreeMap;

use camino::Utf8Path;
use serde::de::Error as _;

pub(crate) fn read_layer(path: &Utf8Path) -> Result<toml::Table, crate::config::ConfigError> {
    let contents = std::fs::read_to_string(path.as_std_path())?;
    let value: toml::Value =
        toml::from_str(&contents).map_err(|source| crate::config::ConfigError::LayerParse {
            path: path.to_path_buf(),
            source,
        })?;

    match value {
        toml::Value::Table(table) => Ok(table),
        _ => Err(crate::config::ConfigError::LayerParse {
            path: path.to_path_buf(),
            source: toml::de::Error::custom("layer root must be a TOML table"),
        }),
    }
}

pub(crate) fn deep_merge(mut earlier: toml::Table, later: toml::Table) -> toml::Table {
    for (key, later_value) in later {
        match (earlier.remove(&key), later_value) {
            (Some(toml::Value::Table(earlier_table)), toml::Value::Table(later_table)) => {
                earlier.insert(
                    key,
                    toml::Value::Table(deep_merge(earlier_table, later_table)),
                );
            }
            (_, replacement) => {
                earlier.insert(key, replacement);
            }
        }
    }
    earlier
}

pub(crate) fn extract_env(
    merged: &mut toml::Table,
) -> Result<BTreeMap<String, String>, crate::config::ConfigError> {
    let Some(env_value) = merged.remove("env") else {
        return Ok(BTreeMap::new());
    };

    let toml::Value::Table(table) = env_value else {
        return Err(crate::config::ConfigError::EnvKeyInvalid {
            key: "env".to_owned(),
            reason: "`[env]` must be a table of string values".to_owned(),
        });
    };

    let mut env = BTreeMap::new();
    for (key, value) in table {
        if !is_valid_env_key(&key) {
            return Err(crate::config::ConfigError::EnvKeyInvalid {
                key,
                reason: "env keys must match ^[A-Za-z_][A-Za-z0-9_]*$".to_owned(),
            });
        }
        if key.starts_with("CODEX_SESSION_") {
            return Err(crate::config::ConfigError::EnvKeyInvalid {
                key,
                reason: "CODEX_SESSION_* keys are wrapper-private and may not be injected"
                    .to_owned(),
            });
        }
        let toml::Value::String(value) = value else {
            return Err(crate::config::ConfigError::EnvKeyInvalid {
                key,
                reason: "env values must be string scalars".to_owned(),
            });
        };
        env.insert(key, value);
    }
    Ok(env)
}

pub(crate) fn is_valid_env_key(key: &str) -> bool {
    let mut chars = key.chars();
    matches!(chars.next(), Some(ch) if ch.is_ascii_alphabetic() || ch == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

/// Reject the legacy profile shapes that codex v0.134+ no longer accepts.
///
/// Called on every input layer (base config layers AND profile files) and on
/// the emitted base `config.toml` as a final bug net. The wrapper's contract
/// is that its output mirrors codex's input — see `docs/upstream-codex.md`
/// §F6b–§F6c and `CLAUDE.md` § Codex config compatibility.
pub(crate) fn reject_legacy_profile_syntax(
    table: &toml::Table,
    location: &str,
) -> Result<(), crate::config::ConfigError> {
    if table.contains_key("profile") {
        return Err(crate::config::ConfigError::LegacyProfileSyntax {
            location: location.to_owned(),
            reason: "top-level `profile = \"...\"` selectors are no longer accepted \
                at any layer; profile selection lives on the codex CLI (`--profile \
                <name>`) against a sibling `profiles/<name>.config.toml` file"
                .to_owned(),
        });
    }
    if table.contains_key("profiles") {
        return Err(crate::config::ConfigError::LegacyProfileSyntax {
            location: location.to_owned(),
            reason: "`[profiles.<name>]` tables are no longer accepted at any layer; \
                each profile's keys must live at the top level of a \
                `profiles/<name>.config.toml` file (no `[profiles.<name>]` \
                header)"
                .to_owned(),
        });
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn parse_table(input: &str) -> toml::Table {
        toml::from_str(input).expect("valid TOML table")
    }

    #[test]
    fn reject_legacy_profile_syntax_flags_top_level_profile_key() {
        let err = reject_legacy_profile_syntax(
            &parse_table("profile = \"deep\"\nmodel = \"x\"\n"),
            "test layer",
        )
        .expect_err("legacy profile selector should fail");

        let crate::config::ConfigError::LegacyProfileSyntax { location, reason } = err else {
            panic!("unexpected error variant");
        };
        assert_eq!(location, "test layer");
        assert!(reason.starts_with("top-level `profile = \"...\"` selectors"));
        assert!(reason.contains("profile selection lives on the codex CLI"));
    }

    #[test]
    fn reject_legacy_profile_syntax_flags_profiles_table() {
        let err =
            reject_legacy_profile_syntax(&parse_table("[profiles.deep]\nmodel = \"x\"\n"), "test")
                .expect_err("legacy profiles table should fail");

        let crate::config::ConfigError::LegacyProfileSyntax { location, reason } = err else {
            panic!("unexpected error variant");
        };
        assert_eq!(location, "test");
        assert!(reason.starts_with("`[profiles.<name>]` tables are"));
        assert!(reason.contains("each profile's keys must live at the top level"));
    }

    #[test]
    fn reject_legacy_profile_syntax_accepts_clean_table() {
        reject_legacy_profile_syntax(&parse_table("model = \"x\"\n"), "clean")
            .expect("clean table should pass");
    }

    #[test]
    fn reject_legacy_profile_syntax_reports_profile_first_when_both_present() {
        let err = reject_legacy_profile_syntax(
            &parse_table("profile = \"deep\"\n[profiles.deep]\nmodel = \"x\"\n"),
            "both",
        )
        .expect_err("legacy profile selector should fail first");

        let crate::config::ConfigError::LegacyProfileSyntax { location, reason } = err else {
            panic!("unexpected error variant");
        };
        assert_eq!(location, "both");
        assert!(reason.starts_with("top-level `profile = \"...\"` selectors"));
    }
}
