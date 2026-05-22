//! `account add` command.
#![allow(clippy::missing_errors_doc, clippy::result_large_err)]

pub(crate) fn run(
    ctx: &crate::context::AppContext,
    args: &crate::cli::account::AccountAddArgs,
) -> Result<(), crate::error::AppError> {
    let registry = crate::services::account::registry::Registry::from_config(&ctx.config);
    let entry = registry.add(&args.name, args.from_native, ctx.home_dir())?;
    tracing::info!(op = "account.add", outcome = "ok", account = %args.name);
    ctx.ui.write_account_mutation(
        "added",
        &crate::commands::account::AccountMutationView {
            name: entry.id.to_string(),
            path: entry.dir,
            archived_to: None,
        },
    )?;
    Ok(())
}
