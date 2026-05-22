//! `account remove` command.
#![allow(clippy::missing_errors_doc, clippy::result_large_err)]

pub(crate) fn run(
    ctx: &crate::context::AppContext,
    args: &crate::cli::account::AccountRemoveArgs,
) -> Result<(), crate::error::AppError> {
    let registry = crate::services::account::registry::Registry::from_config(&ctx.config);
    let path = registry.account_dir(&args.name);
    let archived_to = registry.remove(&args.name)?;
    tracing::info!(op = "account.remove", outcome = "ok", account = %args.name);
    ctx.ui.write_account_mutation(
        "removed",
        &crate::commands::account::AccountMutationView {
            name: args.name.to_string(),
            path,
            archived_to: Some(archived_to),
        },
    )?;
    Ok(())
}
