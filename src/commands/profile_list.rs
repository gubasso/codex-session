//! `profile list` command.
//!
//! What this is: profile-directory inspection and summary rendering.
//! What this is not: profile composition or child execution.
#![allow(clippy::missing_errors_doc, clippy::result_large_err)]

use camino::Utf8PathBuf;

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct ProfileListView {
    pub(crate) profiles: Vec<ProfileListEntry>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct ProfileListEntry {
    pub(crate) name: String,
    pub(crate) manifest_path: Utf8PathBuf,
    pub(crate) layer_count: usize,
    pub(crate) valid: bool,
    pub(crate) error: Option<String>,
}

pub(crate) fn run(
    ctx: &crate::context::AppContext,
    args: crate::cli::profile::ProfileListArgs,
) -> Result<(), crate::error::AppError> {
    let view = build_view(ctx)?;
    ctx.ui.write_profile_list(&view, args.format)?;
    Ok(())
}

fn build_view(ctx: &crate::context::AppContext) -> Result<ProfileListView, crate::error::AppError> {
    let mut profiles = Vec::new();
    let dir = &ctx.config.profile.profiles_dir;
    if !dir.is_dir() {
        return Ok(ProfileListView { profiles });
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
        match crate::services::profile::Manifest::parse(path.clone()) {
            Ok(manifest) => profiles.push(ProfileListEntry {
                name,
                manifest_path: path,
                layer_count: manifest.settings_layers.len(),
                valid: true,
                error: None,
            }),
            Err(err) => profiles.push(ProfileListEntry {
                name,
                manifest_path: path,
                layer_count: 0,
                valid: false,
                error: Some(err.to_string()),
            }),
        }
    }

    profiles.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(ProfileListView { profiles })
}
