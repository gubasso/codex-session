//! `config-recipe list` command.
//!
//! What this is: config-recipe directory inspection and summary rendering.
//! What this is not: config-recipe composition or child execution.
#![allow(clippy::missing_errors_doc, clippy::result_large_err)]

use camino::Utf8PathBuf;

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct ConfigRecipeListView {
    pub(crate) recipes: Vec<ConfigRecipeListEntry>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct ConfigRecipeListEntry {
    pub(crate) name: String,
    pub(crate) manifest_path: Utf8PathBuf,
    pub(crate) layer_count: usize,
    pub(crate) valid: bool,
    pub(crate) error: Option<String>,
}

pub(crate) fn run(
    ctx: &crate::context::AppContext,
    args: crate::cli::config_recipe::ConfigRecipeListArgs,
) -> Result<(), crate::error::AppError> {
    let view = build_view(ctx)?;
    ctx.ui.write_config_recipe_list(&view, args.format)?;
    Ok(())
}

fn build_view(
    ctx: &crate::context::AppContext,
) -> Result<ConfigRecipeListView, crate::error::AppError> {
    let mut recipes = Vec::new();
    let dir = &ctx.config.config_recipe.recipes_dir;
    if !dir.is_dir() {
        return Ok(ConfigRecipeListView { recipes });
    }

    for entry in std::fs::read_dir(dir.as_std_path())? {
        let entry = entry?;
        let path = Utf8PathBuf::try_from(entry.path()).map_err(crate::config::ConfigError::from)?;
        if path.extension().is_none_or(|ext| ext != "yaml") {
            continue;
        }
        let Some(name) = path.file_stem().map(ToOwned::to_owned) else {
            continue;
        };
        match crate::services::config_recipe::Manifest::parse(path.clone()) {
            Ok(manifest) => recipes.push(ConfigRecipeListEntry {
                name,
                manifest_path: path,
                layer_count: manifest.settings_layers.len(),
                valid: true,
                error: None,
            }),
            Err(err) => recipes.push(ConfigRecipeListEntry {
                name,
                manifest_path: path,
                layer_count: 0,
                valid: false,
                error: Some(err.to_string()),
            }),
        }
    }

    recipes.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(ConfigRecipeListView { recipes })
}
