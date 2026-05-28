//! `config-recipe show` command.
//!
//! What this is: manifest and layer inspection for one config-recipe.
//! What this is not: session writing or child exec.
#![allow(clippy::missing_errors_doc, clippy::result_large_err)]

use camino::Utf8PathBuf;

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct ConfigRecipeShowView {
    pub(crate) stock_mode: bool,
    pub(crate) active_config_recipe: Option<String>,
    pub(crate) manifest_path: Option<Utf8PathBuf>,
    pub(crate) layer_paths: Vec<ConfigRecipeLayerView>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct ConfigRecipeLayerView {
    pub(crate) name: String,
    pub(crate) path: Utf8PathBuf,
    pub(crate) exists: bool,
}

pub(crate) fn run(
    ctx: &crate::context::AppContext,
    args: &crate::cli::config_recipe::ConfigRecipeShowArgs,
) -> Result<(), crate::error::AppError> {
    let view = build_view(ctx, args.name.as_deref())?;
    ctx.ui.write_config_recipe_show(&view, args.format)?;
    Ok(())
}

pub(crate) fn build_view(
    ctx: &crate::context::AppContext,
    requested_name: Option<&str>,
) -> Result<ConfigRecipeShowView, crate::error::AppError> {
    let Some(name) = requested_name
        .map(ToOwned::to_owned)
        .or_else(|| ctx.config.config_recipe.active.clone())
    else {
        return Ok(ConfigRecipeShowView {
            stock_mode: true,
            active_config_recipe: None,
            manifest_path: None,
            layer_paths: Vec::new(),
        });
    };

    let manifest_path =
        crate::services::config_recipe::resolve(Some(&name), &ctx.config.config_recipe.recipes_dir)
            .ok_or_else(|| crate::config::ConfigError::ConfigRecipeNotFound {
                name: name.clone(),
                path: ctx
                    .config
                    .config_recipe
                    .recipes_dir
                    .join(format!("{name}.yaml")),
            })?;
    if !manifest_path.is_file() {
        return Err(crate::config::ConfigError::ConfigRecipeNotFound {
            name,
            path: manifest_path,
        }
        .into());
    }

    let manifest = crate::services::config_recipe::Manifest::parse(manifest_path.clone())?;
    let layer_paths = manifest
        .config_layers
        .into_iter()
        .map(|layer_name| {
            let path = ctx
                .config
                .config_recipe
                .configs_dir
                .join(format!("{layer_name}.toml"));
            ConfigRecipeLayerView {
                name: layer_name,
                exists: path.is_file(),
                path,
            }
        })
        .collect();

    Ok(ConfigRecipeShowView {
        stock_mode: false,
        active_config_recipe: Some(name),
        manifest_path: Some(manifest_path),
        layer_paths,
    })
}
