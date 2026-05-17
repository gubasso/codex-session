//! Pass-through command path.
#![allow(clippy::missing_errors_doc)]

use crate::adapters::fs::Fs as _;
use crate::adapters::process::Process as _;

/// Run the non-`self` path.
pub fn run(
    ctx: &crate::context::AppContext,
    argv: &[std::ffi::OsString],
) -> Result<(), crate::error::AppError> {
    let real_codex = ctx
        .process
        .resolve_codex()
        .ok_or(crate::error::AppError::CodexNotFound)?;

    if ctx.fs.exists(&ctx.paths.base)
        && crate::services::merge::needs_merge_raw(&ctx.fs, &ctx.paths)?
    {
        crate::services::merge::perform_merge(&ctx.fs, &ctx.paths)?;
    }

    let err = ctx.process.exec_replace(&real_codex, argv);
    Err(crate::error::AppError::Io(err))
}
