//! Profile settings layer parsing and merge logic.
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

fn is_valid_env_key(key: &str) -> bool {
    let mut chars = key.chars();
    matches!(chars.next(), Some(ch) if ch.is_ascii_alphabetic() || ch == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}
