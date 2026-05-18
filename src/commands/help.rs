//! `help` command.
//!
//! What this is: the explicit top-level `help` verb.
//! What this is not: clap's `--help` flag handler.

const HELP_TEXT: &str = include_str!("../ui/help.txt");

/// Print root help.
pub(crate) fn run(ctx: &crate::context::AppContext) -> Result<(), crate::error::AppError> {
    tracing::info!(op = "help", status = "start");
    ctx.ui.print_help_raw(HELP_TEXT)?;
    tracing::info!(op = "help", status = "ok");
    Ok(())
}
