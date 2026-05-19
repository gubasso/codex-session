//! `profile show` command.
//!
//! What this is: manifest and layer inspection for one profile.
//! What this is not: session writing or child exec.
#![allow(clippy::missing_errors_doc, clippy::result_large_err)]

use camino::Utf8PathBuf;

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct ProfileShowView {
    pub(crate) stock_mode: bool,
    pub(crate) active_profile: Option<String>,
    pub(crate) manifest_path: Option<Utf8PathBuf>,
    pub(crate) layer_paths: Vec<ProfileLayerView>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct ProfileLayerView {
    pub(crate) name: String,
    pub(crate) path: Utf8PathBuf,
    pub(crate) exists: bool,
}

pub(crate) fn run(
    ctx: &crate::context::AppContext,
    args: &crate::cli::profile::ProfileShowArgs,
) -> Result<(), crate::error::AppError> {
    let view = build_view(ctx, args.name.as_deref())?;
    ctx.ui.write_profile_show(&view, args.format)?;
    Ok(())
}

pub(crate) fn build_view(
    ctx: &crate::context::AppContext,
    requested_name: Option<&str>,
) -> Result<ProfileShowView, crate::error::AppError> {
    let Some(name) = requested_name
        .map(ToOwned::to_owned)
        .or_else(|| ctx.config.profile.active.clone())
    else {
        return Ok(ProfileShowView {
            stock_mode: true,
            active_profile: None,
            manifest_path: None,
            layer_paths: Vec::new(),
        });
    };

    let manifest_path =
        crate::services::profile::resolve(Some(&name), &ctx.config.profile.profiles_dir)
            .ok_or_else(|| crate::config::ConfigError::ProfileNotFound {
                name: name.clone(),
                path: ctx.config.profile.profiles_dir.join(format!("{name}.yaml")),
            })?;
    if !manifest_path.is_file() {
        return Err(crate::config::ConfigError::ProfileNotFound {
            name,
            path: manifest_path,
        }
        .into());
    }

    let manifest = crate::services::profile::Manifest::parse(manifest_path.clone())?;
    let layer_paths = manifest
        .settings_layers
        .into_iter()
        .map(|layer_name| {
            let path = ctx
                .config
                .profile
                .settings_dir
                .join(format!("{layer_name}.toml"));
            ProfileLayerView {
                name: layer_name,
                exists: path.is_file(),
                path,
            }
        })
        .collect();

    Ok(ProfileShowView {
        stock_mode: false,
        active_profile: Some(name),
        manifest_path: Some(manifest_path),
        layer_paths,
    })
}
