//! `account current` command.
#![allow(clippy::missing_errors_doc, clippy::result_large_err)]

pub(crate) fn run(
    ctx: &crate::context::AppContext,
    args: crate::cli::account::AccountCurrentArgs,
) -> Result<(), crate::error::AppError> {
    let display = crate::services::account::resolver::resolve_for_display(ctx)?;
    let (name, source) = match display {
        crate::services::account::resolver::DisplayAccount::Pinned { id, source } => (
            id.to_string(),
            crate::services::account::resolver::source_label(source).to_owned(),
        ),
        crate::services::account::resolver::DisplayAccount::Auto {
            last_selected: Some(id),
        } => (id.to_string(), "auto".to_owned()),
        crate::services::account::resolver::DisplayAccount::Auto {
            last_selected: None,
        } => ("(auto — none selected yet)".to_owned(), "auto".to_owned()),
    };
    ctx.ui.write_account_current(
        &crate::commands::account::AccountCurrentView { name, source },
        args.format,
    )?;
    Ok(())
}
