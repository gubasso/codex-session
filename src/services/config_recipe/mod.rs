//! Config-recipe composition services.
//!
//! What this is: pure manifest/layer composition plus session-artifact writing.
//! What this is not: CLI parsing or child-process execution.

#![allow(clippy::result_large_err)]

pub(crate) mod composition;
pub(crate) mod layer;
pub(crate) mod manifest;

pub(crate) use composition::{
    Composition, ConfigRecipePaths, LayerRef, LayerSource, ProfileFileRef, write_session_artifacts,
    write_stock_session_artifacts,
};
pub(crate) use layer::{
    deep_merge, extract_env, is_valid_env_key, read_layer, reject_legacy_profile_syntax,
};
pub(crate) use manifest::Manifest;

use camino::{Utf8Path, Utf8PathBuf};

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

    if let Some(cache_config) = paths.cache_config.as_ref()
        && cache_config.is_file()
    {
        let cache_layer = read_layer(cache_config)?;
        reject_legacy_profile_syntax(&cache_layer, "cache layer")?;
        merged = deep_merge(merged, cache_layer);
        layer_refs.push(LayerRef {
            name: "configs".to_owned(),
            path: cache_config.clone(),
            source: LayerSource::CacheBootstrap,
        });
    }

    for layer_name in &manifest.config_layers {
        let layer_path = paths.configs_dir.join(format!("{layer_name}.toml"));
        if !layer_path.is_file() {
            return Err(crate::config::ConfigError::LayerNotFound {
                name: layer_name.clone(),
                path: layer_path,
            });
        }
        let layer = read_layer(&layer_path)?;
        reject_legacy_profile_syntax(&layer, &format!("config layer `{layer_name}.toml`"))?;
        merged = deep_merge(merged, layer);
        layer_refs.push(LayerRef {
            name: layer_name.clone(),
            path: layer_path,
            source: LayerSource::ConfigRecipe,
        });
    }

    let profile_files = collect_profile_files(&manifest, paths)?;
    let env = extract_env(&mut merged)?;
    // Bug-net: defend against future merge logic that re-introduces top-level
    // `profile` / `profiles` keys. The per-layer and per-file checks above
    // already cover normal input; this re-check runs against the post-merge,
    // post-`extract_env` surface so a refactor of either path cannot silently
    // smuggle the legacy shape into the emitted base config.
    reject_legacy_profile_syntax(&merged, "merged base config")?;
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
        profile_files,
    })
}

fn collect_profile_files(
    manifest: &Manifest,
    paths: &ConfigRecipePaths,
) -> Result<Vec<ProfileFileRef>, crate::config::ConfigError> {
    let profiles_dir = paths.profiles_dir();
    let names: Vec<String> = match manifest.profile_files.as_ref() {
        Some(declared) => declared.clone(),
        None => scan_profile_files(&profiles_dir)?,
    };

    let mut refs = Vec::with_capacity(names.len());
    for name in names {
        let path = profiles_dir.join(format!("{name}.config.toml"));
        if !path.is_file() {
            return Err(crate::config::ConfigError::ProfileFileNotFound { name, path });
        }
        let raw_toml =
            std::fs::read_to_string(path.as_std_path()).map_err(crate::config::ConfigError::Io)?;
        let table: toml::Table =
            toml::from_str(&raw_toml).map_err(|source| crate::config::ConfigError::LayerParse {
                path: path.clone(),
                source,
            })?;
        reject_legacy_profile_syntax(&table, &format!("profile file `{name}.config.toml`"))?;
        refs.push(ProfileFileRef {
            name,
            path,
            raw_toml,
        });
    }
    Ok(refs)
}

fn scan_profile_files(dir: &Utf8Path) -> Result<Vec<String>, crate::config::ConfigError> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    for entry in std::fs::read_dir(dir.as_std_path())? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let Ok(name_os) = entry.file_name().into_string() else {
            continue;
        };
        let Some(stem) = name_os.strip_suffix(".config.toml") else {
            // Non-profile file in the directory (e.g. README, .gitkeep);
            // ignore. Only the `*.config.toml` suffix is in scope.
            continue;
        };
        if !manifest::is_valid_layer_name(stem) {
            // Fail loudly: the directory-scan contract is "emit every
            // *.config.toml". Silently dropping a file with an invalid stem
            // would let codex see one set of profiles and the operator see
            // another. Surface the bad name with a clear path so the user
            // can rename or remove it.
            return Err(crate::config::ConfigError::InvalidProfileFileName {
                name: stem.to_owned(),
                path: dir.join(&name_os),
            });
        }
        names.push(stem.to_owned());
    }
    names.sort();
    Ok(names)
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
