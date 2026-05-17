//! `self version` command.
#![allow(clippy::missing_errors_doc)]

/// Print the wrapper version.
pub fn run(ctx: &crate::context::AppContext) -> Result<(), crate::error::AppError> {
    ctx.ui.print_version(crate::domain::version::current())?;
    Ok(())
}
