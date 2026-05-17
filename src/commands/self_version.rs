//! `self version` command.
#![allow(clippy::missing_errors_doc)]

/// Print the wrapper version.
pub(crate) fn run(
    ctx: &crate::context::AppContext,
    _args: crate::cli::self_version::SelfVersionArgs,
) -> Result<(), crate::error::AppError> {
    ctx.ui.print_version(crate::domain::version::current())?;
    Ok(())
}
