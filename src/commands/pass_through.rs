//! Pass-through command path.
//!
//! What this is: the handler for forwarded child invocations.
//! What this is not: clap parsing or child process execution primitives.
#![allow(clippy::missing_errors_doc, clippy::result_large_err)]

use crate::adapters::fs::Fs as _;
use crate::adapters::spawner::Spawner as _;
use crate::domain::child_invocation::{ChildEnv, ChildInvocation};

/// Run the non-`self` path.
pub(crate) fn run(
    ctx: &crate::context::AppContext,
    argv: &[std::ffi::OsString],
) -> Result<(), crate::error::AppError> {
    tracing::info!(op = "pass-through", status = "start", argc = argv.len());
    let resolved = ctx.resolved_child().map_err(|err| match err {
        crate::adapters::spawner::SpawnerError::NotFound {
            tried,
            path_searched,
        } => crate::error::AppError::ChildNotFound {
            tried: tried.clone().into_std_path_buf(),
            path_searched: path_searched.clone(),
        },
        crate::adapters::spawner::SpawnerError::NotExecutable { path } => {
            crate::error::AppError::ChildNotExecutable {
                path: path.clone().into_std_path_buf(),
            }
        }
        crate::adapters::spawner::SpawnerError::Recursion { path } => {
            crate::error::AppError::ChildRecursion { path: path.clone() }
        }
        crate::adapters::spawner::SpawnerError::Exec(io) => {
            crate::error::AppError::ChildExec(std::io::Error::new(io.kind(), io.to_string()))
        }
        crate::adapters::spawner::SpawnerError::NonUtf8Path(_) => {
            crate::error::AppError::Other(anyhow::anyhow!("non-utf8 child path"))
        }
    })?;
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
    Err(crate::error::AppError::from(err))
}
