//! Pass-through command path.
#![allow(clippy::missing_errors_doc)]

use crate::adapters::fs::Fs as _;
use crate::adapters::process::Process as _;

/// Run the non-`self` path.
pub(crate) fn run(
    ctx: &crate::context::AppContext,
    argv: &[std::ffi::OsString],
) -> Result<(), crate::error::AppError> {
    tracing::info!(op = "pass-through", status = "start", argc = argv.len());
    let real_codex = ctx
        .process
        .resolve_codex(ctx.config.child.bin.as_deref())
        .map_err(crate::error::AppError::from_process_error)?;
    tracing::info!(
        op = "child.resolve",
        status = "ok",
        bin.resolved = %real_codex.display()
    );

    if ctx.fs.exists(ctx.paths().base_config.as_std_path())
        && crate::services::merge::needs_merge_raw(&ctx.fs, ctx.paths())?
    {
        crate::services::merge::perform_merge(&ctx.fs, ctx.paths())?;
    }

    let err = ctx.process.exec_replace(&real_codex, argv);
    Err(crate::error::AppError::Process(err))
}
