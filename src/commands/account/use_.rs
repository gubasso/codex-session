//! `account use` command.
#![allow(clippy::missing_errors_doc, clippy::result_large_err)]

pub(crate) fn run(
    ctx: &crate::context::AppContext,
    args: &crate::cli::account::AccountUseArgs,
) -> Result<(), crate::error::AppError> {
    let registry = crate::services::account::registry::Registry::from_config(&ctx.config);
    registry.set_current(&args.name)?;
    tracing::info!(op = "account.use", outcome = "ok", account = %args.name);
    ctx.ui.write_account_mutation(
        "selected",
        &crate::commands::account::AccountMutationView {
            verb: "selected",
            name: args.name.to_string(),
            path: registry.account_dir(&args.name),
        },
        args.format,
    )?;
    Ok(())
}
