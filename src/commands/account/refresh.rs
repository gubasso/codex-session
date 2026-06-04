//! `account refresh` command.
#![allow(clippy::missing_errors_doc, clippy::result_large_err)]

use std::io::IsTerminal as _;

use crate::ui::spinner::{SpinnerGroup, should_show_spinner};

pub(crate) fn run(
    ctx: &crate::context::AppContext,
    args: &crate::cli::account::AccountRefreshArgs,
) -> Result<(), crate::error::AppError> {
    if !std::io::stdin().is_terminal() {
        return Err(crate::services::account::AccountError::NonInteractive {
            action: "account refresh".to_owned(),
        }
        .into());
    }

    let registry = crate::services::account::registry::Registry::from_config(&ctx.config);
    let name = match &args.name {
        Some(name) => name.clone(),
        None => registry
            .current()?
            .ok_or(crate::services::account::AccountError::NoEligible { report: Vec::new() })?,
    };
    let spinners = SpinnerGroup::new(should_show_spinner(ctx, args.format, false));
    let spinner = spinners.add(&format!("Preparing login for \"{name}\"..."));
    let _ = registry.expect_account_dir(&name)?;
    spinner.finish_and_clear_for_child();

    let (_dir, auth_path) = super::run_isolated_login(ctx)?;

    let spinner = spinners.add("Saving credentials...");
    super::persist_auth_to_seed(&auth_path, &registry, &name)?;
    registry.delete_group_auths(&name)?;
    spinner.finish_and_clear();

    tracing::info!(op = "account.refresh", outcome = "ok", account = %name);
    ctx.ui.write_account_mutation(
        "refreshed",
        &crate::commands::account::AccountMutationView {
            verb: "refreshed",
            name: name.to_string(),
            path: registry.account_dir(&name),
        },
        args.format,
    )?;
    Ok(())
}
