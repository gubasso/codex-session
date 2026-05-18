//! `self config-status` command.
#![allow(clippy::missing_errors_doc)]

use crate::adapters::fs::Fs as _;
use crate::adapters::process::Process as _;

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct ConfigStatusReport {
    pub(crate) base_path: String,
    pub(crate) base_exists: bool,
    pub(crate) target_path: String,
    pub(crate) target_exists: bool,
    pub(crate) stamp_path: String,
    pub(crate) stamp_exists: bool,
    pub(crate) needs_merge: bool,
    pub(crate) child_bin: Option<String>,
    pub(crate) log_file: String,
}

/// Print the config merge status.
pub(crate) fn run(
    ctx: &crate::context::AppContext,
    args: crate::cli::self_config_status::SelfConfigStatusArgs,
) -> Result<(), crate::error::AppError> {
    tracing::info!(op = "self.config-status", status = "start");
    let base_exists = ctx.fs.exists(&ctx.paths.base);
    let target_exists = ctx.fs.exists(&ctx.paths.target);
    let stamp_exists = ctx.fs.exists(&ctx.paths.stamp);
    let needs_merge = crate::services::merge::needs_merge_observed(&ctx.fs, &ctx.paths)?;
    let report = ConfigStatusReport {
        base_path: ctx.paths.base.display().to_string(),
        base_exists,
        target_path: ctx.paths.target.display().to_string(),
        target_exists,
        stamp_path: ctx.paths.stamp.display().to_string(),
        stamp_exists,
        needs_merge,
        child_bin: ctx
            .process
            .resolve_codex()
            .ok()
            .map(|path| path.display().to_string()),
        log_file: ctx.paths.log_file.display().to_string(),
    };
    match args.format {
        crate::cli::OutputFormat::Text => ctx.ui.print_config_status(&report)?,
        crate::cli::OutputFormat::Json => ctx.ui.print_json(&report)?,
    }
    tracing::info!(op = "self.config-status", status = "ok");
    Ok(())
}
