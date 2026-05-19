//! `version` command.
#![allow(clippy::missing_errors_doc)]

use crate::adapters::spawner::Spawner as _;

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
    args: crate::cli::version::VersionArgs,
) -> Result<(), crate::error::AppError> {
    tracing::info!(op = "version", status = "start");
    let child_path = ctx.resolved_child().ok().cloned();
    let report = VersionReport {
        wrapper_version: crate::domain::version::current().to_owned(),
        child_version: child_path
            .as_deref()
            .and_then(|path| ctx.spawner.child_version_line(path)),
        child_path: child_path.map(|path| path.to_string()),
    };
    match args.format {
        crate::cli::OutputFormat::Text => ctx.ui.print_version_details(&report)?,
        crate::cli::OutputFormat::Json => ctx.ui.print_json(&report)?,
    }
    tracing::info!(op = "version", status = "ok");
    Ok(())
}
