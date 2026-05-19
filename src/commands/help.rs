//! `help` command.
//!
//! What this is: the explicit top-level `help` verb.
//! What this is not: clap's `--help` flag handler.

#![allow(clippy::result_large_err)]

const HELP_TEXT: &str = include_str!("../ui/help.txt");

/// Print root help.
pub(crate) fn run(ctx: &crate::context::AppContext) -> Result<(), crate::error::AppError> {
    tracing::info!(op = "help", status = "start");
    ctx.ui.write_help(HELP_TEXT)?;
    tracing::info!(op = "help", status = "ok");
    Ok(())
}
