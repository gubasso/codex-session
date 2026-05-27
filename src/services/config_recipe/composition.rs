//! Composition data structures and session-artifact writing.
//!
//! What this is: the serializable output of config-recipe composition.
//! What this is not: active config-recipe resolution or command rendering.

#![allow(clippy::result_large_err)]

use std::collections::BTreeMap;
use std::io::Write as _;

use camino::{Utf8Path, Utf8PathBuf};
use serde::Serialize;

#[derive(Debug, Clone)]
pub(crate) struct ConfigRecipePaths {
    pub(crate) recipes_dir: Utf8PathBuf,
    pub(crate) settings_dir: Utf8PathBuf,
    pub(crate) cache_settings: Option<Utf8PathBuf>,
}

#[derive(Debug, Clone)]
pub(crate) struct Composition {
    pub(crate) manifest_path: Utf8PathBuf,
    pub(crate) layer_paths: Vec<LayerRef>,
    pub(crate) merged_config: toml::Table,
    pub(crate) env: BTreeMap<String, String>,
    /// Snapshot of the `[projects]` table from the merged source config,
    /// captured after `[env]` extraction. The post-flight trust sync diffs
    /// the post-session `[projects]` against this baseline to find new
    /// trust decisions written by codex during the run.
    pub(crate) baseline_projects: Option<toml::Table>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct LayerRef {
    pub(crate) name: String,
    pub(crate) path: Utf8PathBuf,
    pub(crate) source: LayerSource,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum LayerSource {
    CacheBootstrap,
    ConfigRecipe,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
struct ComposeSidecar<'a> {
    manifest: &'a Utf8Path,
    layers: &'a [LayerRef],
    env: &'a BTreeMap<String, String>,
    /// Compose-time snapshot of `[projects]` from source layers, serialized
    /// for observability only — the post-flight trust sync receives the
    /// same data in-memory via `Composition::baseline_projects` and does
    /// **not** read this sidecar field back. `null` when no source layer
    /// declares any projects entries.
    baseline_projects: Option<serde_json::Value>,
}

pub(crate) fn write_session_artifacts(
    composition: &Composition,
    session_dir: &Utf8Path,
) -> Result<(), crate::config::ConfigError> {
    std::fs::create_dir_all(session_dir.as_std_path())?;

    let config_path = session_dir.join("config.toml");
    let sidecar_path = session_dir.join(".codex-session-compose.json");

    let config_string = toml::to_string_pretty(&composition.merged_config).map_err(|err| {
        crate::config::ConfigError::MergeFailed {
            reason: err.to_string(),
        }
    })?;
    write_atomic(&config_path, &config_string)?;

    let sidecar = ComposeSidecar {
        manifest: composition.manifest_path.as_ref(),
        layers: &composition.layer_paths,
        env: &composition.env,
        baseline_projects: baseline_projects_as_json(composition.baseline_projects.as_ref())?,
    };
    let sidecar_string = serde_json::to_string_pretty(&sidecar).map_err(|err| {
        crate::config::ConfigError::MergeFailed {
            reason: err.to_string(),
        }
    })?;
    write_atomic(&sidecar_path, &sidecar_string)?;

    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
struct StockComposeSidecar {
    manifest: Option<Utf8PathBuf>,
    layers: Vec<LayerRef>,
    env: BTreeMap<String, String>,
    /// Always `null` in stock mode (no source layers, no baseline).
    baseline_projects: Option<serde_json::Value>,
}

pub(crate) fn write_stock_session_artifacts(
    session_dir: &Utf8Path,
) -> Result<(), crate::config::ConfigError> {
    std::fs::create_dir_all(session_dir.as_std_path())?;
    write_atomic(&session_dir.join("config.toml"), "")?;
    // Stable sidecar shape so consumers can read one schema in both modes.
    let sidecar = StockComposeSidecar {
        manifest: None,
        layers: Vec::new(),
        env: BTreeMap::new(),
        baseline_projects: None,
    };
    let sidecar_string = serde_json::to_string_pretty(&sidecar).map_err(|err| {
        crate::config::ConfigError::MergeFailed {
            reason: err.to_string(),
        }
    })?;
    write_atomic(
        &session_dir.join(".codex-session-compose.json"),
        &sidecar_string,
    )?;
    Ok(())
}

/// Convert an optional `toml::Table` into a `serde_json::Value` for sidecar
/// serialization. `toml::Table` does not derive `Serialize` into JSON for
/// arbitrary nesting, so we route via `toml::Value -> serde_json::Value`.
fn baseline_projects_as_json(
    table: Option<&toml::Table>,
) -> Result<Option<serde_json::Value>, crate::config::ConfigError> {
    let Some(table) = table else { return Ok(None) };
    let value = toml::Value::Table(table.clone());
    let json =
        serde_json::to_value(value).map_err(|err| crate::config::ConfigError::MergeFailed {
            reason: format!("baseline-projects sidecar serialization: {err}"),
        })?;
    Ok(Some(json))
}

pub(crate) fn write_atomic(
    path: &Utf8Path,
    contents: &str,
) -> Result<(), crate::config::ConfigError> {
    let Some(parent) = path.parent() else {
        return Err(crate::config::ConfigError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "target has no parent",
        )));
    };

    std::fs::create_dir_all(parent.as_std_path())?;
    let mut temp = tempfile::Builder::new()
        .prefix(".codex-session.")
        .tempfile_in(parent.as_std_path())?;
    temp.write_all(contents.as_bytes())?;
    temp.persist(path.as_std_path())
        .map_err(|err| crate::config::ConfigError::Io(err.error))?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn baseline_projects_round_trip_through_json() {
        // Nested `[projects."/a/b"]` tables must survive
        // toml::Table -> serde_json::Value -> toml::Table without losing
        // structure or content. The sidecar is observability-only — the
        // post-flight trust sync uses `Composition::baseline_projects` in
        // memory — but operators and downstream tooling that read the
        // sidecar JSON still rely on this round-trip being lossless.
        let mut inner = toml::Table::new();
        inner.insert(
            "trust_level".to_owned(),
            toml::Value::String("trusted".to_owned()),
        );
        let mut projects = toml::Table::new();
        projects.insert("/a/b".to_owned(), toml::Value::Table(inner));
        projects.insert(
            "/c d/e".to_owned(),
            toml::Value::Table({
                let mut t = toml::Table::new();
                t.insert(
                    "trust_level".to_owned(),
                    toml::Value::String("untrusted".to_owned()),
                );
                t
            }),
        );

        let json = baseline_projects_as_json(Some(&projects))
            .expect("convert to json")
            .expect("Some");
        let value: toml::Value =
            serde_json::from_value::<toml::Value>(json).expect("json -> toml::Value");
        let parsed = value.as_table().expect("table").clone();
        assert_eq!(parsed, projects);
    }

    #[test]
    fn baseline_projects_none_is_none() {
        let result = baseline_projects_as_json(None).expect("ok");
        assert!(result.is_none());
    }
}
