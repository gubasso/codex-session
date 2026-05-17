//! Pass-through command path.
#![allow(clippy::missing_errors_doc)]

use crate::adapters::fs::Fs as _;
use crate::adapters::process::Process as _;

/// Run the non-`self` path.
pub(crate) fn run(
    ctx: &crate::context::AppContext,
    argv: &[std::ffi::OsString],
) -> Result<(), crate::error::AppError> {
    tracing::debug!(?argv, "passing through to real codex");
    let real_codex = match ctx.process.resolve_codex() {
        Ok(path) => path,
        Err(crate::adapters::process::ProcessError::CodexNotFound) => {
            return Err(crate::error::AppError::CodexNotFound);
        }
        Err(err) => return Err(crate::error::AppError::Process(err)),
    };
    tracing::debug!(real_codex = %real_codex.display(), "resolved real codex");

    if ctx.fs.exists(&ctx.paths.base)
        && crate::services::merge::needs_merge_raw(&ctx.fs, &ctx.paths)?
    {
        crate::services::merge::perform_merge(&ctx.fs, &ctx.paths)?;
    }

    let err = ctx.process.exec_replace(&real_codex, argv);
    Err(crate::error::AppError::Process(err))
}
