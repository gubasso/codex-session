//! `config-recipe compose` command.
//!
//! What this is: explicit session-dir composition for inspection.
//! What this is not: pass-through exec.
#![allow(clippy::missing_errors_doc, clippy::result_large_err)]

use camino::Utf8PathBuf;

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct ConfigRecipeComposeView {
    pub(crate) stock_mode: bool,
    pub(crate) config_recipe: Option<String>,
    pub(crate) group_id: String,
    pub(crate) session_dir: Utf8PathBuf,
    pub(crate) config_path: Utf8PathBuf,
    pub(crate) sidecar_path: Utf8PathBuf,
    pub(crate) session_meta_path: Utf8PathBuf,
}

pub(crate) fn run(
    ctx: &crate::context::AppContext,
    args: crate::cli::config_recipe::ConfigRecipeComposeArgs,
) -> Result<(), crate::error::AppError> {
    let session = ctx.session()?;
    let group_id = session.group_id.as_str().to_owned();
    let session_dir = session.dir.clone();
    let cwd = current_cwd()?;

    let config_recipe = args
        .name
        .or_else(|| ctx.config.config_recipe.active.clone());
    if let Some(name) = config_recipe.as_deref() {
        let composition = crate::services::config_recipe::compose(
            name,
            &crate::services::config_recipe::ConfigRecipePaths {
                recipes_dir: ctx.config.config_recipe.recipes_dir.clone(),
                settings_dir: ctx.config.config_recipe.settings_dir.clone(),
                cache_settings: cache_settings_path(ctx),
            },
        )?;
        crate::services::config_recipe::write_session_artifacts(&composition, &session_dir)?;
        let meta = crate::services::session::meta::SessionMeta::new(
            Some(name),
            &group_id,
            cwd.as_ref(),
            "(compose)",
            "compose",
        );
        crate::services::session::meta::write(&session_dir, &meta)?;
        let view = build_view(Some(name.to_owned()), false, group_id, session_dir);
        ctx.ui.write_config_recipe_compose(&view)?;
    } else {
        crate::services::config_recipe::write_stock_session_artifacts(&session_dir)?;
        let meta = crate::services::session::meta::SessionMeta::new(
            None,
            &group_id,
            cwd.as_ref(),
            "(compose)",
            "compose",
        );
        crate::services::session::meta::write(&session_dir, &meta)?;
        let view = build_view(None, true, group_id, session_dir);
        ctx.ui.write_config_recipe_compose(&view)?;
    }

    Ok(())
}

fn build_view(
    config_recipe: Option<String>,
    stock_mode: bool,
    group_id: String,
    session_dir: Utf8PathBuf,
) -> ConfigRecipeComposeView {
    ConfigRecipeComposeView {
        stock_mode,
        config_recipe,
        group_id,
        config_path: session_dir.join("config.toml"),
        sidecar_path: session_dir.join(".codex-session-compose.json"),
        session_meta_path: session_dir.join("session-meta.json"),
        session_dir,
    }
}

fn current_cwd() -> Result<Utf8PathBuf, crate::config::ConfigError> {
    Utf8PathBuf::try_from(std::env::current_dir().map_err(crate::config::ConfigError::CurrentDir)?)
        .map_err(crate::config::ConfigError::from)
}

fn cache_settings_path(ctx: &crate::context::AppContext) -> Option<Utf8PathBuf> {
    let path = ctx.config.paths.cache_dir.join("settings.toml");
    path.is_file().then_some(path)
}
