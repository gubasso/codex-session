//! Composition data structures and session-artifact writing.
//!
//! What this is: the serializable output of profile composition.
//! What this is not: active-profile resolution or command rendering.

#![allow(clippy::result_large_err)]

use std::collections::BTreeMap;
use std::io::Write as _;

use camino::{Utf8Path, Utf8PathBuf};
use serde::Serialize;

#[derive(Debug, Clone)]
pub(crate) struct ProfilePaths {
    pub(crate) profiles_dir: Utf8PathBuf,
    pub(crate) settings_dir: Utf8PathBuf,
    pub(crate) cache_settings: Option<Utf8PathBuf>,
}

#[derive(Debug, Clone)]
pub(crate) struct Composition {
    pub(crate) manifest_path: Utf8PathBuf,
    pub(crate) layer_paths: Vec<LayerRef>,
    pub(crate) merged_config: toml::Table,
    pub(crate) env: BTreeMap<String, String>,
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
    Profile,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
struct ComposeSidecar<'a> {
    manifest: &'a Utf8Path,
    layers: &'a [LayerRef],
    env: &'a BTreeMap<String, String>,
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
}

pub(crate) fn write_stock_session_artifacts(
    session_dir: &Utf8Path,
) -> Result<(), crate::config::ConfigError> {
    std::fs::create_dir_all(session_dir.as_std_path())?;
    write_atomic(&session_dir.join("config.toml"), "")?;
    // Stable sidecar shape so consumers can read one schema in both modes.
    // Plan Phase 12, Stage 1 acceptance: `{"manifest":null,"layers":[],"env":{}}`.
    let sidecar = StockComposeSidecar {
        manifest: None,
        layers: Vec::new(),
        env: BTreeMap::new(),
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
