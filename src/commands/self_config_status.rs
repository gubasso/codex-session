//! `self config-status` command.
#![allow(clippy::missing_errors_doc)]

use crate::adapters::fs::Fs as _;

/// Print the config merge status.
pub(crate) fn run(
    ctx: &crate::context::AppContext,
    _args: crate::cli::self_config_status::SelfConfigStatusArgs,
) -> Result<(), crate::error::AppError> {
    let base_exists = ctx.fs.exists(&ctx.paths.base);
    let target_exists = ctx.fs.exists(&ctx.paths.target);
    let stamp_exists = ctx.fs.exists(&ctx.paths.stamp);
    let needs_merge = crate::services::merge::needs_merge_observed(&ctx.fs, &ctx.paths)?;
    ctx.ui.print_config_status(
        &ctx.paths,
        base_exists,
        target_exists,
        stamp_exists,
        needs_merge,
    )?;
    Ok(())
}
