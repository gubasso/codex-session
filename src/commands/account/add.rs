//! `account add` command.
#![allow(clippy::missing_errors_doc, clippy::result_large_err)]

use std::io::IsTerminal as _;

pub(crate) fn run(
    ctx: &crate::context::AppContext,
    args: &crate::cli::account::AccountAddArgs,
) -> Result<(), crate::error::AppError> {
    let registry = crate::services::account::registry::Registry::from_config(&ctx.config);

    if !std::io::stdin().is_terminal() {
        return Err(crate::services::account::AccountError::NonInteractive {
            action: "account add".to_owned(),
        }
        .into());
    }

    let entry = registry.add(&args.name)?;

    let (_dir, auth_path) = match super::run_isolated_login(ctx) {
        Ok(result) => result,
        Err(err) => {
            let _ = registry.remove(&args.name).inspect_err(|cleanup_err| {
                tracing::warn!(
                    op = "account.add",
                    outcome = "cleanup-failed",
                    account = %args.name,
                    error = %cleanup_err
                );
            });
            return Err(err);
        }
    };

    super::persist_auth_to_seed(&auth_path, &registry, &args.name)?;
    registry.set_current(&args.name)?;

    tracing::info!(op = "account.add", outcome = "ok", account = %args.name);
    ctx.ui.write_account_mutation(
        "added",
        &crate::commands::account::AccountMutationView {
            verb: "added",
            name: entry.id.to_string(),
            path: entry.dir,
        },
        args.format,
    )?;
    Ok(())
}
