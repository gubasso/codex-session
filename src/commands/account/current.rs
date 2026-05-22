//! `account current` command.
#![allow(clippy::missing_errors_doc, clippy::result_large_err)]

pub(crate) fn run(
    ctx: &crate::context::AppContext,
    args: crate::cli::account::AccountCurrentArgs,
) -> Result<(), crate::error::AppError> {
    let resolved = crate::services::account::resolver::resolve(ctx)?;
    ctx.ui.write_account_current(
        &crate::commands::account::AccountCurrentView {
            name: resolved.id.to_string(),
            source: crate::services::account::resolver::source_label(resolved.source).to_owned(),
        },
        args.format,
    )?;
    Ok(())
}
