//! `self help` command.
#![allow(clippy::missing_errors_doc)]

const HELP_TEXT: &str = include_str!("../ui/self_help.txt");

/// Print the wrapper help text.
pub fn run(ctx: &crate::context::AppContext) -> Result<(), crate::error::AppError> {
    ctx.ui.print_help_raw(HELP_TEXT)?;
    Ok(())
}
