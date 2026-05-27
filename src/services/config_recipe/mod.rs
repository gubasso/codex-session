//! Config-recipe composition services.
//!
//! What this is: pure manifest/layer composition plus session-artifact writing.
//! What this is not: CLI parsing or child-process execution.

#![allow(clippy::result_large_err)]

pub(crate) mod composition;
pub(crate) mod layer;
pub(crate) mod manifest;

pub(crate) use composition::{
    Composition, ConfigRecipePaths, LayerRef, LayerSource, write_session_artifacts,
    write_stock_session_artifacts,
};
pub(crate) use layer::{deep_merge, extract_env, is_valid_env_key, read_layer};
pub(crate) use manifest::Manifest;

use camino::Utf8PathBuf;

/// Compose a named config-recipe without writing any session artifacts.
pub(crate) fn compose(
    recipe_name: &str,
    paths: &ConfigRecipePaths,
) -> Result<Composition, crate::config::ConfigError> {
    let manifest_path = paths.recipes_dir.join(format!("{recipe_name}.yaml"));
    if !manifest_path.is_file() {
        return Err(crate::config::ConfigError::ConfigRecipeNotFound {
            name: recipe_name.to_owned(),
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
            source: LayerSource::ConfigRecipe,
        });
    }

    let env = extract_env(&mut merged)?;
    // Snapshot AFTER `extract_env`, which mutates `merged` by removing
    // `[env]`. The baseline must reflect what eventually gets serialized
    // into the session config so the post-flight trust sync can diff
    // accurately.
    let baseline_projects = merged.get("projects").and_then(|v| v.as_table().cloned());
    Ok(Composition {
        manifest_path: manifest.path,
        layer_paths: layer_refs,
        merged_config: merged,
        env,
        baseline_projects,
    })
}

/// Resolve the named config-recipe manifest path if it exists.
#[allow(
    clippy::manual_map,
    clippy::option_if_let_else,
    clippy::single_option_map
)]
pub(crate) fn resolve(
    recipe_name: Option<&str>,
    recipes_dir: &camino::Utf8Path,
) -> Option<Utf8PathBuf> {
    match recipe_name {
        Some(name) => Some(recipes_dir.join(format!("{name}.yaml"))),
        None => None,
    }
}
