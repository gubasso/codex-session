//! `config status` command.
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
    pub(crate) log_verbose: u8,
    pub(crate) log_mirror_stderr: bool,
    pub(crate) log_format: crate::config::LogFormat,
    pub(crate) sources: ConfigStatusSources,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct ConfigStatusSources {
    pub(crate) defaults: bool,
    pub(crate) user: Option<String>,
    pub(crate) project: Option<String>,
    pub(crate) env: String,
    pub(crate) cli: String,
}

/// Print the config merge status.
pub(crate) fn run(
    ctx: &crate::context::AppContext,
    args: crate::cli::config::ConfigStatusArgs,
) -> Result<(), crate::error::AppError> {
    tracing::info!(op = "config.status", status = "start");
    let stamp = ctx.paths().stamp_file();
    let base_exists = ctx.fs.exists(ctx.paths().base_config.as_std_path());
    let target_exists = ctx.fs.exists(ctx.paths().target_config.as_std_path());
    let stamp_exists = ctx.fs.exists(stamp.as_std_path());
    let needs_merge = crate::services::merge::needs_merge_observed(&ctx.fs, ctx.paths())?;
    let report = ConfigStatusReport {
        base_path: ctx.paths().base_config.to_string(),
        base_exists,
        target_path: ctx.paths().target_config.to_string(),
        target_exists,
        stamp_path: stamp.to_string(),
        stamp_exists,
        needs_merge,
        child_bin: ctx
            .process
            .resolve_codex(ctx.config.child.bin.as_deref())
            .ok()
            .map(|path| path.display().to_string()),
        log_file: ctx
            .config
            .log
            .file
            .clone()
            .unwrap_or_else(|| ctx.paths().state_dir.join("codex-session.log"))
            .to_string(),
        log_verbose: ctx.config.log.verbose,
        log_mirror_stderr: ctx.config.log.mirror_stderr,
        log_format: ctx.config.log.format,
        sources: ConfigStatusSources {
            defaults: true,
            user: ctx.config.sources.user.as_ref().map(ToString::to_string),
            project: ctx.config.sources.project.as_ref().map(ToString::to_string),
            env: ctx.config.sources.env_prefix.to_owned(),
            cli: ctx.config.sources.cli.clone(),
        },
    };
    match args.format {
        crate::cli::OutputFormat::Text => ctx.ui.print_config_status(&report)?,
        crate::cli::OutputFormat::Json => ctx.ui.print_json(&report)?,
    }
    tracing::info!(op = "config.status", status = "ok");
    Ok(())
}
