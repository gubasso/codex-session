//! `self version` command.
#![allow(clippy::missing_errors_doc)]

use crate::adapters::process::Process as _;

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct VersionReport {
    pub(crate) wrapper_version: String,
    pub(crate) child_path: Option<String>,
    pub(crate) child_version: Option<String>,
}

/// Print the wrapper version.
pub(crate) fn run(
    ctx: &crate::context::AppContext,
    args: crate::cli::self_version::SelfVersionArgs,
) -> Result<(), crate::error::AppError> {
    tracing::info!(op = "self.version", status = "start");
    let child_path = ctx.process.resolve_codex().ok();
    let report = VersionReport {
        wrapper_version: crate::domain::version::current().to_owned(),
        child_version: child_path
            .as_ref()
            .and_then(|path| ctx.process.child_version_line(path)),
        child_path: child_path.map(|path| path.display().to_string()),
    };
    match args.format {
        crate::cli::OutputFormat::Text => ctx.ui.print_version_details(&report)?,
        crate::cli::OutputFormat::Json => ctx.ui.print_json(&report)?,
    }
    tracing::info!(op = "self.version", status = "ok");
    Ok(())
}
