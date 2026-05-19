//! Pass-through command path.
#![allow(clippy::missing_errors_doc)]

use crate::adapters::fs::Fs as _;
use crate::adapters::spawner::Spawner as _;
use crate::domain::child_invocation::{ChildEnv, ChildInvocation};

/// Run the non-`self` path.
pub(crate) fn run(
    ctx: &crate::context::AppContext,
    argv: &[std::ffi::OsString],
) -> Result<(), crate::error::AppError> {
    tracing::info!(op = "pass-through", status = "start", argc = argv.len());
    let resolved = ctx
        .resolved_child()
        .map_err(crate::error::AppError::from_spawner_error_ref)?;
    tracing::info!(
        op = "child.resolve",
        status = "ok",
        bin.resolved = %resolved
    );

    let inv = ChildInvocation {
        binary: resolved.clone(),
        args: argv.to_vec(),
        env: ChildEnv::scrubbed_default(),
    };

    if ctx.global.dry_run {
        ctx.ui.write_dry_run(&inv.dry_run_report())?;
        tracing::info!(op = "pass-through", status = "ok", outcome = "dry-run");
        return Ok(());
    }

    if ctx.fs.exists(ctx.paths().base_config.as_std_path())
        && crate::services::merge::needs_merge_raw(&ctx.fs, ctx.paths())?
    {
        crate::services::merge::perform_merge(&ctx.fs, ctx.paths())?;
    }

    let err = ctx.spawner.exec(inv);
    Err(crate::error::AppError::from_spawner_error(err))
}
