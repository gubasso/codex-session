//! `account list` command.
#![allow(clippy::missing_errors_doc, clippy::result_large_err)]

pub(crate) fn run(
    ctx: &crate::context::AppContext,
    args: crate::cli::account::AccountListArgs,
) -> Result<(), crate::error::AppError> {
    let registry = crate::services::account::registry::Registry::from_config(&ctx.config);
    // Propagate resolution errors so an invalid CODEX_SESSION_ACCOUNT (or a
    // malformed `state/last-account`) surfaces with the documented exit code
    // (`account_invalid_env_exits_64`), rather than being silently dropped
    // by `.ok()` and showing an incomplete list.
    let resolved = crate::services::account::resolver::resolve(ctx)?;
    let active = Some(crate::commands::account::AccountCurrentView {
        name: resolved.id.to_string(),
        source: crate::services::account::resolver::source_label(resolved.source).to_owned(),
    });
    let active_name = active.as_ref().map(|value| value.name.as_str());
    let entries = registry
        .list()?
        .into_iter()
        .map(|entry| crate::commands::account::AccountListEntryView {
            current: active_name == Some(entry.id.as_str()),
            last_used_at_unix: crate::commands::account::as_unix(entry.last_used_at),
            name: entry.id.to_string(),
            dir: entry.dir,
            has_auth: entry.has_auth,
        })
        .collect();
    ctx.ui.write_account_list(
        &crate::commands::account::AccountListView {
            active,
            accounts: entries,
        },
        args.format,
    )?;
    Ok(())
}
