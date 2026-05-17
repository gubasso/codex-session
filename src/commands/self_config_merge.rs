//! `self config-merge` command.
#![allow(clippy::missing_errors_doc)]

use crate::adapters::fs::Fs as _;

/// Force a merge regardless of stamp freshness.
pub(crate) fn run(
    ctx: &crate::context::AppContext,
    _args: crate::cli::self_config_merge::SelfConfigMergeArgs,
) -> Result<(), crate::error::AppError> {
    if !ctx.fs.exists(&ctx.paths.base) {
        return Err(crate::error::AppError::BaseMissing(ctx.paths.base.clone()));
    }
    crate::services::merge::perform_merge(&ctx.fs, &ctx.paths)?;
    ctx.ui
        .print_merge_success(&ctx.paths.base, &ctx.paths.target)?;
    Ok(())
}
