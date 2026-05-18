//! `self help` command.
#![allow(clippy::missing_errors_doc)]

const HELP_TEXT: &str = include_str!("../ui/self_help.txt");

/// Print the wrapper help text.
pub(crate) fn run(
    ctx: &crate::context::AppContext,
    _args: crate::cli::self_help::SelfHelpArgs,
) -> Result<(), crate::error::AppError> {
    tracing::info!(op = "self.help", status = "start");
    ctx.ui.print_help_raw(HELP_TEXT)?;
    tracing::info!(op = "self.help", status = "ok");
    Ok(())
}
