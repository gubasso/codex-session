//! `account list` command.
#![allow(clippy::missing_errors_doc, clippy::result_large_err)]

pub(crate) fn run(
    ctx: &crate::context::AppContext,
    args: crate::cli::account::AccountListArgs,
) -> Result<(), crate::error::AppError> {
    let registry = crate::services::account::registry::Registry::from_config(&ctx.config);
    let active = match crate::services::account::resolver::resolve(ctx) {
        Ok(resolved) => Some(crate::commands::account::AccountCurrentView {
            name: resolved.id.to_string(),
            source: crate::services::account::resolver::source_label(resolved.source).to_owned(),
        }),
        Err(crate::error::AppError::Account(
            crate::services::account::AccountError::NoneResolved,
        )) => None,
        Err(err) => return Err(err),
    };
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
