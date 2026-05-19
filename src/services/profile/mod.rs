//! Profile composition services.
//!
//! What this is: pure manifest/layer composition plus session-artifact writing.
//! What this is not: CLI parsing or child-process execution.

#![allow(clippy::result_large_err)]

pub(crate) mod composition;
pub(crate) mod layer;
pub(crate) mod manifest;

pub(crate) use composition::{
    Composition, LayerRef, LayerSource, ProfilePaths, write_session_artifacts,
    write_stock_session_artifacts,
};
pub(crate) use layer::{deep_merge, extract_env, read_layer};
pub(crate) use manifest::Manifest;

use camino::Utf8PathBuf;

/// Compose a named profile without writing any session artifacts.
pub(crate) fn compose(
    profile_name: &str,
    paths: &ProfilePaths,
) -> Result<Composition, crate::config::ConfigError> {
    let manifest_path = paths.profiles_dir.join(format!("{profile_name}.yaml"));
    if !manifest_path.is_file() {
        return Err(crate::config::ConfigError::ProfileNotFound {
            name: profile_name.to_owned(),
            path: manifest_path,
        });
    }

    let manifest = Manifest::parse(manifest_path)?;
    let mut merged = toml::Table::new();
    let mut layer_refs = Vec::new();

    if let Some(cache_settings) = paths.cache_settings.as_ref()
        && cache_settings.is_file()
    {
        let cache_layer = read_layer(cache_settings)?;
        merged = deep_merge(merged, cache_layer);
        layer_refs.push(LayerRef {
            name: "settings".to_owned(),
            path: cache_settings.clone(),
            source: LayerSource::CacheBootstrap,
        });
    }

    for layer_name in &manifest.settings_layers {
        let layer_path = paths.settings_dir.join(format!("{layer_name}.toml"));
        if !layer_path.is_file() {
            return Err(crate::config::ConfigError::LayerNotFound {
                name: layer_name.clone(),
                path: layer_path,
            });
        }
        let layer = read_layer(&layer_path)?;
        merged = deep_merge(merged, layer);
        layer_refs.push(LayerRef {
            name: layer_name.clone(),
            path: layer_path,
            source: LayerSource::Profile,
        });
    }

    let env = extract_env(&mut merged)?;
    Ok(Composition {
        manifest_path: manifest.path,
        layer_paths: layer_refs,
        merged_config: merged,
        env,
    })
}

/// Resolve the named profile manifest path if it exists.
#[allow(
    clippy::manual_map,
    clippy::option_if_let_else,
    clippy::single_option_map
)]
pub(crate) fn resolve(
    profile_name: Option<&str>,
    profiles_dir: &camino::Utf8Path,
) -> Option<Utf8PathBuf> {
    match profile_name {
        Some(name) => Some(profiles_dir.join(format!("{name}.yaml"))),
        None => None,
    }
}
