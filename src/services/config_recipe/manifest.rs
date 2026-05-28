//! Config-recipe manifest parsing.
//!
//! What this is: YAML parsing and schema validation for `config-recipes/*.yaml`.
//! What this is not: TOML layer parsing or merge logic.

#![allow(clippy::result_large_err)]

use camino::Utf8PathBuf;
use serde::Deserialize;

#[derive(Debug, Clone)]
pub(crate) struct Manifest {
    pub(crate) path: Utf8PathBuf,
    pub(crate) config_layers: Vec<String>,
    pub(crate) profile_files: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    #[serde(rename = "config-layers")]
    config_layers: serde_yaml_ng::Value,
    #[serde(rename = "profile-files", default)]
    profile_files: Option<serde_yaml_ng::Value>,
}

impl Manifest {
    pub(crate) fn parse(path: Utf8PathBuf) -> Result<Self, crate::config::ConfigError> {
        let contents = std::fs::read_to_string(path.as_std_path())?;
        let raw: RawManifest = serde_yaml_ng::from_str(&contents).map_err(|source| {
            crate::config::ConfigError::ManifestParse {
                path: path.clone(),
                source,
            }
        })?;

        let layers_value = raw.config_layers;
        let serde_yaml_ng::Value::Sequence(layers) = layers_value else {
            return Err(crate::config::ConfigError::ManifestSchema {
                path,
                reason: "`config-layers` must be a non-empty array of strings".to_owned(),
            });
        };

        if layers.is_empty() {
            return Err(crate::config::ConfigError::ManifestSchema {
                path,
                reason: "`config-layers` must not be empty".to_owned(),
            });
        }

        let mut config_layers = Vec::with_capacity(layers.len());
        for item in layers {
            let serde_yaml_ng::Value::String(name) = item else {
                return Err(crate::config::ConfigError::ManifestSchema {
                    path,
                    reason: "`config-layers` entries must be strings".to_owned(),
                });
            };
            if !is_valid_layer_name(&name) {
                return Err(crate::config::ConfigError::ManifestSchema {
                    path,
                    reason: format!("invalid config layer name `{name}`"),
                });
            }
            config_layers.push(name);
        }

        let profile_files = match raw.profile_files {
            None => None,
            Some(serde_yaml_ng::Value::Sequence(items)) => {
                if items.is_empty() {
                    return Err(crate::config::ConfigError::ManifestSchema {
                        path,
                        reason: "`profile-files` must not be empty when present".to_owned(),
                    });
                }
                let mut names = Vec::with_capacity(items.len());
                let mut seen = std::collections::BTreeSet::new();
                for item in items {
                    let serde_yaml_ng::Value::String(name) = item else {
                        return Err(crate::config::ConfigError::ManifestSchema {
                            path,
                            reason: "`profile-files` entries must be strings".to_owned(),
                        });
                    };
                    if !is_valid_layer_name(&name) {
                        return Err(crate::config::ConfigError::ManifestSchema {
                            path,
                            reason: format!("invalid profile-file name `{name}`"),
                        });
                    }
                    if !seen.insert(name.clone()) {
                        return Err(crate::config::ConfigError::ManifestSchema {
                            path,
                            reason: format!("duplicate profile-file name `{name}`"),
                        });
                    }
                    names.push(name);
                }
                Some(names)
            }
            Some(_) => {
                return Err(crate::config::ConfigError::ManifestSchema {
                    path,
                    reason: "`profile-files` must be a non-empty array of strings".to_owned(),
                });
            }
        };

        Ok(Self {
            path,
            config_layers,
            profile_files,
        })
    }
}

/// Validate a layer / profile-file stem against the `[a-z0-9._-]+` regex
/// the round-02 contract pins for both `config-layers:` and
/// `profile-files:` entries. ASCII uppercase is intentionally rejected so
/// that the on-disk filename, the YAML reference, and the emitted sibling
/// name agree case-for-case on every platform (some filesystems are
/// case-insensitive, which would silently alias `Deep.config.toml` and
/// `deep.config.toml` and let codex see two profiles where the operator
/// declared one).
pub(crate) fn is_valid_layer_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().all(|ch| {
            ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '.' | '_' | '-')
        })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::is_valid_layer_name;

    #[test]
    fn accepts_lowercase_digits_and_punctuation() {
        assert!(is_valid_layer_name("base"));
        assert!(is_valid_layer_name("base-work"));
        assert!(is_valid_layer_name("v1.2"));
        assert!(is_valid_layer_name("a_b"));
        assert!(is_valid_layer_name("0"));
    }

    #[test]
    fn rejects_empty() {
        assert!(!is_valid_layer_name(""));
    }

    #[test]
    fn rejects_uppercase() {
        // The round-02 contract is `[a-z0-9._-]+`. Uppercase must be
        // rejected so case-insensitive filesystems cannot silently alias
        // `Deep.config.toml` and `deep.config.toml` into two profiles
        // where the operator declared one.
        assert!(!is_valid_layer_name("Deep"));
        assert!(!is_valid_layer_name("DEEP"));
        assert!(!is_valid_layer_name("baseWork"));
    }

    #[test]
    fn rejects_whitespace_and_other_punctuation() {
        assert!(!is_valid_layer_name("base work"));
        assert!(!is_valid_layer_name("base/work"));
        assert!(!is_valid_layer_name("base:work"));
    }
}
