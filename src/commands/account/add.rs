//! `account add` command.
#![allow(clippy::missing_errors_doc, clippy::result_large_err)]

use std::io::IsTerminal as _;

pub(crate) fn run(
    ctx: &crate::context::AppContext,
    args: &crate::cli::account::AccountAddArgs,
) -> Result<(), crate::error::AppError> {
    let registry = crate::services::account::registry::Registry::from_config(&ctx.config);

    if args.from_current {
        let entry = registry.add(&args.name, true, ctx.home_dir())?;
        registry.set_current(&args.name)?;
        tracing::info!(op = "account.add", outcome = "ok", account = %args.name);
        ctx.ui.write_account_mutation(
            "added",
            &crate::commands::account::AccountMutationView {
                name: entry.id.to_string(),
                path: entry.dir,
            },
        )?;
        return Ok(());
    }

    if !std::io::stdin().is_terminal() {
        return Err(crate::services::account::AccountError::NonInteractive {
            action: "account add".to_owned(),
        }
        .into());
    }

    let entry = registry.add(&args.name, false, ctx.home_dir())?;

    let _ = super::spawn_child(ctx, ["logout"]).inspect_err(|err| {
        tracing::warn!(
            op = "account.add",
            outcome = "logout-failed-non-fatal",
            error = %err
        );
    });

    if let Err(err) = super::spawn_child(ctx, ["login"]) {
        let _ = registry.remove(&args.name).inspect_err(|cleanup_err| {
            tracing::warn!(
                op = "account.add",
                outcome = "cleanup-failed",
                account = %args.name,
                error = %cleanup_err
            );
        });
        return Err(crate::services::account::AccountError::LoginFailed {
            detail: err.to_string(),
        }
        .into());
    }

    super::copy_native_auth_to_seed(ctx, &registry, &args.name)?;
    registry.set_current(&args.name)?;

    tracing::info!(op = "account.add", outcome = "ok", account = %args.name);
    ctx.ui.write_account_mutation(
        "added",
        &crate::commands::account::AccountMutationView {
            name: entry.id.to_string(),
            path: entry.dir,
        },
    )?;
    Ok(())
}
