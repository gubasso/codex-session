//! `config merge` command.
//!
//! What this is: the forced config-merge verb handler.
//! What this is not: the merge algorithm itself; that lives in
//! `services::merge`.
#![allow(clippy::missing_errors_doc, clippy::result_large_err)]

use crate::adapters::fs::Fs as _;

/// Force a merge regardless of stamp freshness.
pub(crate) fn run(
    ctx: &crate::context::AppContext,
    _args: crate::cli::config::ConfigMergeArgs,
) -> Result<(), crate::error::AppError> {
    tracing::info!(op = "config.merge", status = "start");
    if !ctx.fs.exists(ctx.paths().base_config.as_std_path()) {
        return Err(crate::error::AppError::BaseMissing(
            ctx.paths().base_config.clone().into_std_path_buf(),
        ));
    }
    crate::services::merge::perform_merge(&ctx.fs, ctx.paths())?;
    ctx.ui.print_merge_success(
        ctx.paths().base_config.as_std_path(),
        ctx.paths().target_config.as_std_path(),
    )?;
    tracing::info!(op = "config.merge", status = "ok");
    Ok(())
}
