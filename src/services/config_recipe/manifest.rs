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
    pub(crate) settings_layers: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    #[serde(rename = "settings-layers")]
    settings_layers: serde_yaml_ng::Value,
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

        let layers_value = raw.settings_layers;
        let serde_yaml_ng::Value::Sequence(layers) = layers_value else {
            return Err(crate::config::ConfigError::ManifestSchema {
                path,
                reason: "`settings-layers` must be a non-empty array of strings".to_owned(),
            });
        };

        if layers.is_empty() {
            return Err(crate::config::ConfigError::ManifestSchema {
                path,
                reason: "`settings-layers` must not be empty".to_owned(),
            });
        }

        let mut settings_layers = Vec::with_capacity(layers.len());
        for item in layers {
            let serde_yaml_ng::Value::String(name) = item else {
                return Err(crate::config::ConfigError::ManifestSchema {
                    path,
                    reason: "`settings-layers` entries must be strings".to_owned(),
                });
            };
            if !is_valid_layer_name(&name) {
                return Err(crate::config::ConfigError::ManifestSchema {
                    path,
                    reason: format!("invalid settings layer name `{name}`"),
                });
            }
            settings_layers.push(name);
        }

        Ok(Self {
            path,
            settings_layers,
        })
    }
}

fn is_valid_layer_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
}
