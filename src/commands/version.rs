//! `version` command.
//!
//! What this is: the verb handler that prints wrapper and resolved child
//! version details.
//! What this is not: the clap parse shape; that lives in `cli::version`.
#![allow(clippy::missing_errors_doc, clippy::result_large_err)]

use crate::adapters::spawner::Spawner as _;

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct VersionView {
    /// Wrapper version string.
    pub(crate) wrapper_version: String,
    /// Resolved child binary path when available.
    pub(crate) child_path: Option<String>,
    /// Child version line when it can be probed.
    pub(crate) child_version: Option<String>,
    pub(crate) account: Option<String>,
    pub(crate) account_source: Option<String>,
}

/// Print the wrapper version.
pub(crate) fn run(
    ctx: &crate::context::AppContext,
    args: crate::cli::version::VersionArgs,
) -> Result<(), crate::error::AppError> {
    tracing::info!(op = "version", status = "start");
    let view = build_view(ctx);
    ctx.ui.write_version(&view, args.format)?;
    tracing::info!(op = "version", status = "ok");
    Ok(())
}

pub(crate) fn build_view(ctx: &crate::context::AppContext) -> VersionView {
    let child_path = ctx.resolved_child().ok().cloned();
    let display = crate::services::account::resolver::resolve_for_display(ctx).ok();
    let (account, account_source) = display.map_or((None, None), |display| match display {
        crate::services::account::resolver::DisplayAccount::Pinned { id, source } => (
            Some(id.to_string()),
            Some(crate::services::account::resolver::source_label(source).to_owned()),
        ),
        crate::services::account::resolver::DisplayAccount::Auto {
            last_selected: Some(id),
        } => (Some(id.to_string()), Some("auto".to_owned())),
        crate::services::account::resolver::DisplayAccount::Auto {
            last_selected: None,
        } => (None, Some("auto".to_owned())),
    });
    VersionView {
        wrapper_version: crate::domain::version::current().to_owned(),
        child_version: child_path
            .as_deref()
            .and_then(|path| ctx.spawner.child_version_line(path)),
        child_path: child_path.map(|path| path.to_string()),
        account,
        account_source,
    }
}
