//! `config status` command.
//!
//! What this is: read-only reporting for active config-recipe and session-root state.
//! What this is not: pass-through execution or config-recipe composition writes.
#![allow(clippy::missing_errors_doc, clippy::result_large_err)]

use camino::Utf8PathBuf;

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct ConfigStatusView {
    pub(crate) active_config_recipe: Option<String>,
    pub(crate) manifest_path: Option<Utf8PathBuf>,
    pub(crate) layer_paths: Vec<LayerEntry>,
    pub(crate) account: String,
    pub(crate) account_source: String,
    pub(crate) group_id: String,
    pub(crate) group_id_source: String,
    pub(crate) codex_home: Utf8PathBuf,
    pub(crate) accounts_count: usize,
    pub(crate) active_account_has_auth: bool,
    pub(crate) accounts_in_cooldown: usize,
    pub(crate) session_root: Utf8PathBuf,
    pub(crate) session_root_source: String,
    pub(crate) child_bin: Option<Utf8PathBuf>,
    pub(crate) log: LogView,
    pub(crate) sources: SourcesView,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct LayerEntry {
    pub(crate) name: String,
    pub(crate) path: Utf8PathBuf,
    pub(crate) exists: bool,
    pub(crate) error: Option<String>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct LogView {
    pub(crate) file: String,
    pub(crate) verbose: u8,
    pub(crate) mirror_stderr: bool,
    pub(crate) format: crate::config::LogFormat,
    pub(crate) stderr_format: Option<crate::config::LogFormat>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct SourcesView {
    pub(crate) defaults: bool,
    pub(crate) user: Option<String>,
    pub(crate) project: Option<String>,
    pub(crate) env: String,
    pub(crate) cli: String,
}

pub(crate) fn run(
    ctx: &crate::context::AppContext,
    args: crate::cli::config::ConfigStatusArgs,
) -> Result<(), crate::error::AppError> {
    let view = build_view(ctx)?;
    ctx.ui.write_config_status(&view, args.format)?;
    Ok(())
}

#[allow(clippy::too_many_lines)]
pub(crate) fn build_view(
    ctx: &crate::context::AppContext,
) -> Result<ConfigStatusView, crate::error::AppError> {
    let root = crate::services::session::dir::resolve_session_root(
        ctx.config.paths.runtime_dir.as_deref(),
        &ctx.config.paths.state_dir,
    )?;
    let resolved_group = crate::services::session::group_id::current(ctx)?;
    let resolved_account = match crate::services::account::resolver::resolve_for_display(ctx)? {
        crate::services::account::resolver::DisplayAccount::Pinned { id, source } => {
            Some(crate::services::account::resolver::ResolvedAccount { id, source })
        }
        crate::services::account::resolver::DisplayAccount::Auto {
            last_selected: Some(id),
        } => Some(crate::services::account::resolver::ResolvedAccount {
            id,
            source: crate::services::account::resolver::AccountResolutionSource::Auto,
        }),
        crate::services::account::resolver::DisplayAccount::Auto {
            last_selected: None,
        } => None,
    };
    let inspected_dir = if let Some(ref ra) = resolved_account {
        crate::services::session::dir::inspect_session_dir(
            &root.path,
            &ra.id,
            resolved_group.id.as_str(),
        )?
    } else {
        crate::services::session::dir::InspectedSessionDir {
            path: root.path.clone(),
            missing: true,
        }
    };

    // `config status` is an introspection command: even if the active config-recipe
    // is missing or its manifest is malformed, still report what we resolved
    // and surface the error inline via a synthetic layer entry. The plan
    // (Phase 12, Step D6) locks in this degraded-status behavior.
    let (manifest_path, layer_paths) = ctx.config.config_recipe.active.as_deref().map_or_else(
        || (None, Vec::new()),
        |active_config_recipe| config_recipe_layers_or_error(ctx, active_config_recipe),
    );
    let registry = crate::services::account::registry::Registry::from_config(&ctx.config);
    let account_list = registry.list().unwrap_or_default();
    let accounts_count = account_list.len();
    let active_account_has_auth = resolved_account.as_ref().is_none_or(|ra| {
        account_list
            .iter()
            .find(|a| a.id == ra.id)
            .is_none_or(|a| a.has_auth)
    });
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let accounts_in_cooldown = account_list
        .iter()
        .filter(|a| {
            let root = registry.account_dir(&a.id);
            crate::services::account::cooldown::read(&root)
                .ok()
                .flatten()
                .is_some_and(|cd| crate::services::account::cooldown::is_active(&cd, now_unix))
        })
        .count();

    Ok(ConfigStatusView {
        active_config_recipe: ctx.config.config_recipe.active.clone(),
        manifest_path,
        layer_paths,
        account: resolved_account
            .as_ref()
            .map_or_else(|| "(no account)".to_owned(), |ra| ra.id.to_string()),
        account_source: resolved_account.as_ref().map_or_else(
            || "none".to_owned(),
            |ra| crate::services::account::resolver::source_label(ra.source).to_owned(),
        ),
        group_id: resolved_group.id.as_str().to_owned(),
        group_id_source: group_id_source_label(resolved_group.source).to_owned(),
        codex_home: inspected_dir.path,
        accounts_count,
        active_account_has_auth,
        accounts_in_cooldown,
        session_root: root.path,
        session_root_source: match root.source {
            crate::services::session::dir::SessionRootSource::Runtime => "runtime".to_owned(),
            crate::services::session::dir::SessionRootSource::State => "state".to_owned(),
        },
        child_bin: ctx.resolved_child().ok().cloned(),
        log: LogView {
            file: ctx
                .config
                .log
                .file
                .clone()
                .unwrap_or_else(|| ctx.paths().state_dir.clone())
                .to_string(),
            verbose: ctx.config.log.verbose,
            mirror_stderr: ctx.config.log.mirror_stderr,
            format: ctx.config.log.format,
            stderr_format: ctx.config.log.stderr_format,
        },
        sources: SourcesView {
            defaults: true,
            user: ctx.config.sources.user.as_ref().map(ToString::to_string),
            project: ctx.config.sources.project.as_ref().map(ToString::to_string),
            env: ctx.config.sources.env_prefix.to_owned(),
            cli: ctx.config.sources.cli.clone(),
        },
    })
}

const fn group_id_source_label(
    source: crate::services::session::group_id::GroupIdSource,
) -> &'static str {
    match source {
        crate::services::session::group_id::GroupIdSource::Flag => "flag",
        crate::services::session::group_id::GroupIdSource::Env => "env",
        crate::services::session::group_id::GroupIdSource::Tty => "tty",
        crate::services::session::group_id::GroupIdSource::Ppid => "ppid",
        crate::services::session::group_id::GroupIdSource::Pid => "pid",
    }
}

fn config_recipe_layers_or_error(
    ctx: &crate::context::AppContext,
    active_config_recipe: &str,
) -> (Option<Utf8PathBuf>, Vec<LayerEntry>) {
    let manifest_path = ctx
        .config
        .config_recipe
        .recipes_dir
        .join(format!("{active_config_recipe}.yaml"));

    if !manifest_path.is_file() {
        // Report missing manifest as a synthetic entry so JSON consumers still
        // see the active config-recipe + an error message instead of a hard failure.
        let entry = LayerEntry {
            name: active_config_recipe.to_owned(),
            path: manifest_path.clone(),
            exists: false,
            error: Some(format!("config-recipe `{active_config_recipe}` not found")),
        };
        return (Some(manifest_path), vec![entry]);
    }

    match crate::services::config_recipe::Manifest::parse(manifest_path.clone()) {
        Ok(manifest) => {
            let entries = manifest
                .config_layers
                .into_iter()
                .map(|name| {
                    let path = ctx
                        .config
                        .config_recipe
                        .configs_dir
                        .join(format!("{name}.toml"));
                    LayerEntry {
                        name,
                        exists: path.is_file(),
                        path,
                        error: None,
                    }
                })
                .collect();
            (Some(manifest_path), entries)
        }
        Err(err) => {
            let entry = LayerEntry {
                name: active_config_recipe.to_owned(),
                path: manifest_path.clone(),
                exists: true,
                error: Some(err.to_string()),
            };
            (Some(manifest_path), vec![entry])
        }
    }
}
