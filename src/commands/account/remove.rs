//! `account remove` command.
#![allow(clippy::missing_errors_doc, clippy::result_large_err)]

use std::io::IsTerminal as _;

pub(crate) fn run(
    ctx: &crate::context::AppContext,
    args: &crate::cli::account::AccountRemoveArgs,
) -> Result<(), crate::error::AppError> {
    let registry = crate::services::account::registry::Registry::from_config(&ctx.config);
    let path = registry.account_dir(&args.name);
    if let Ok(Some(ref current_id)) = registry.current()
        && *current_id == args.name
    {
        ctx.ui.write_warning(&format!(
            "warning: '{}' is the current active account; \
            after removal, the next invocation will fall back to account resolution defaults",
            args.name,
        ))?;
    }
    let groups_dir = path.join("groups");
    if let Ok(entries) = std::fs::read_dir(groups_dir.as_std_path()) {
        let now = std::time::SystemTime::now();
        let recent_count = entries
            .flatten()
            .filter(|e| {
                e.metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|mtime| now.duration_since(mtime).ok())
                    .is_some_and(|age| age < std::time::Duration::from_secs(24 * 3600))
            })
            .count();
        if recent_count > 0 {
            ctx.ui.write_warning(&format!(
                "warning: '{}' has {recent_count} session group(s) used in the last 24h; \
                removing it may break in-progress Codex sessions",
                args.name,
            ))?;
        }
    }
    if !args.yes && !std::io::stdin().is_terminal() {
        return Err(crate::services::account::AccountError::NonInteractive {
            action: "account remove".to_owned(),
        }
        .into());
    }
    if !args.yes {
        ctx.ui.write_prompt(&format!(
            "remove account '{}' permanently? [y/N]: ",
            args.name
        ))?;
        let mut answer = String::new();
        std::io::BufRead::read_line(&mut std::io::stdin().lock(), &mut answer)?;
        if !matches!(answer.trim(), "y" | "Y" | "yes" | "YES" | "Yes") {
            return Ok(());
        }
    }
    registry.remove(&args.name)?;
    tracing::info!(op = "account.remove", outcome = "ok", account = %args.name);
    ctx.ui.write_account_mutation(
        "removed",
        &crate::commands::account::AccountMutationView {
            verb: "removed",
            name: args.name.to_string(),
            path,
        },
        args.format,
    )?;
    Ok(())
}
