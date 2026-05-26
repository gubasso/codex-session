//! Auth gate — ensures an account is resolved and authenticated before launch.
//!
//! Also provides `run_login` / `run_logout` for `codex-session login` and
//! `codex-session logout` — these are managed by the gate (not raw
//! pass-throughs) so that every auth operation is account-aware and narrated.
//!
//! For the pass-through path (`ensure`), auth validity is determined by
//! seed-file existence only. `run_login` additionally runs a heartbeat probe
//! to verify the token server-side before skipping re-authentication.
#![allow(clippy::result_large_err)]

use std::io::IsTerminal as _;

use super::{
    AccountError, AccountId,
    registry::{AccountEntry, Registry},
    resolver::{AccountResolutionSource, ResolvedAccount, source_label},
};

use camino::Utf8PathBuf;

use crate::context::AppContext;
use crate::error::AppError;

const PING_PROFILE: &str = "ping";

pub(crate) struct LoginOptions {
    pub(crate) force: bool,
}

impl LoginOptions {
    pub(crate) fn from_argv(tail: &[std::ffi::OsString]) -> Self {
        let mut opts = Self { force: false };
        for arg in tail {
            if let Some("--force" | "-f") = arg.to_str() {
                opts.force = true;
            }
        }
        opts
    }
}

#[derive(Debug)]
pub(crate) enum AccountState {
    Ready(ResolvedAccount),
    AuthMissing { account: AccountId },
    NoneSelected { accounts: Vec<AccountEntry> },
    NoAccounts,
}

pub(crate) fn assess(ctx: &AppContext) -> Result<AccountState, AppError> {
    let registry = Registry::from_config(&ctx.config);
    let accounts = registry.list()?;

    if accounts.is_empty() {
        return Ok(AccountState::NoAccounts);
    }

    match super::resolver::resolve(ctx) {
        Err(AppError::Account(AccountError::NoneResolved)) => {
            Ok(AccountState::NoneSelected { accounts })
        }
        Err(err) => Err(err),
        Ok(resolved) => match registry.expect_account_dir(&resolved.id) {
            Err(AccountError::NotFound { name, .. }) => {
                tracing::warn!(
                    op = "gate.assess",
                    outcome = "stale-pointer",
                    account = %name,
                    "resolved account directory missing; treating as none-selected"
                );
                Ok(AccountState::NoneSelected { accounts })
            }
            Err(err) => Err(err.into()),
            Ok(_) => {
                let seed_path = registry.group_auth_seed_path(&resolved.id);
                if !seed_path.as_std_path().exists() {
                    return Ok(AccountState::AuthMissing {
                        account: resolved.id,
                    });
                }
                Ok(AccountState::Ready(resolved))
            }
        },
    }
}

pub(crate) fn ensure(ctx: &AppContext) -> Result<ResolvedAccount, AppError> {
    let state = assess(ctx)?;
    let interactive = std::io::stdin().is_terminal();

    match state {
        AccountState::Ready(resolved) => {
            narrate(
                ctx,
                &format!(
                    "account '{}' (source: {}), auth present — launching.",
                    resolved.id,
                    source_label(resolved.source),
                ),
            );
            Ok(resolved)
        }
        AccountState::NoAccounts => {
            if !interactive {
                return Err(AccountError::NoAccounts.into());
            }
            narrate(
                ctx,
                "no accounts registered. Let's set up your first account.",
            );
            interactive_resolve_no_accounts(ctx)
        }
        AccountState::NoneSelected { accounts } => {
            if !interactive {
                return Err(AccountError::NoneSelected.into());
            }
            narrate(
                ctx,
                &format!(
                    "{} account(s) found but none is selected. Please choose one.",
                    accounts.len(),
                ),
            );
            interactive_resolve_none_selected(ctx, &accounts)
        }
        AccountState::AuthMissing { account, .. } => {
            if !interactive {
                return Err(AccountError::AuthMissing { name: account }.into());
            }
            warn_and_confirm_auth_missing(ctx, &account)
        }
    }
}

fn warn_and_confirm_auth_missing(
    ctx: &AppContext,
    account: &AccountId,
) -> Result<ResolvedAccount, AppError> {
    let _ = ctx.ui.write_warning(&format!(
        "\nwarning: account '{account}' is selected but has no valid authentication token.\n\
        The token may be missing or expired. You need to re-authenticate before launching.\n",
    ));
    interactive_resolve_auth_missing(ctx, account)
}

fn interactive_resolve_no_accounts(ctx: &AppContext) -> Result<ResolvedAccount, AppError> {
    let account_id = prompt_account_name()?;
    narrate(
        ctx,
        &format!("creating account '{account_id}' and starting authentication..."),
    );
    do_add_account(ctx, &account_id)?;
    narrate(
        ctx,
        &format!("account '{account_id}' created and authenticated — launching."),
    );
    Ok(ResolvedAccount {
        id: account_id,
        source: AccountResolutionSource::Interactive,
    })
}

fn interactive_resolve_none_selected(
    ctx: &AppContext,
    accounts: &[AccountEntry],
) -> Result<ResolvedAccount, AppError> {
    let selected = prompt_select_account(accounts)?;
    match selected {
        AccountSelection::Existing(account_id) => {
            let has_auth = accounts
                .iter()
                .find(|a| a.id == account_id)
                .is_some_and(|a| a.has_auth);
            if !has_auth {
                narrate(
                    ctx,
                    &format!("account '{account_id}' has no authentication token."),
                );
                return interactive_resolve_auth_missing(ctx, &account_id);
            }
            let registry = Registry::from_config(&ctx.config);
            registry.set_current(&account_id)?;
            narrate(
                ctx,
                &format!("account '{account_id}' selected — launching."),
            );
            Ok(ResolvedAccount {
                id: account_id,
                source: AccountResolutionSource::Interactive,
            })
        }
        AccountSelection::AddNew => {
            let account_id = prompt_account_name()?;
            narrate(
                ctx,
                &format!("creating account '{account_id}' and starting authentication..."),
            );
            do_add_account(ctx, &account_id)?;
            narrate(
                ctx,
                &format!("account '{account_id}' created and authenticated — launching."),
            );
            Ok(ResolvedAccount {
                id: account_id,
                source: AccountResolutionSource::Interactive,
            })
        }
    }
}

fn interactive_resolve_auth_missing(
    ctx: &AppContext,
    account: &AccountId,
) -> Result<ResolvedAccount, AppError> {
    let options = vec![
        format!("Re-authenticate '{account}'"),
        "Switch to a different account".to_owned(),
        "Add a new account".to_owned(),
    ];

    let choice = inquire::Select::new(
        &format!("Account '{account}' has no valid authentication. What would you like to do?"),
        options,
    )
    .prompt()
    .map_err(|err| AccountError::NonInteractive {
        action: format!("auth gate prompt: {err}"),
    })?;

    if choice.starts_with("Re-authenticate") {
        narrate(
            ctx,
            &format!(
                "re-authenticating account '{account}' — \
                running the codex native login flow...",
            ),
        );
        do_refresh_auth(ctx, account)?;
        narrate(
            ctx,
            &format!("account '{account}' re-authenticated — launching."),
        );
        Ok(ResolvedAccount {
            id: account.clone(),
            source: AccountResolutionSource::Interactive,
        })
    } else if choice.starts_with("Switch") {
        let registry = Registry::from_config(&ctx.config);
        let accounts = registry.list()?;
        interactive_resolve_none_selected(ctx, &accounts)
    } else {
        let account_id = prompt_account_name()?;
        narrate(
            ctx,
            &format!("creating account '{account_id}' and starting authentication..."),
        );
        do_add_account(ctx, &account_id)?;
        narrate(
            ctx,
            &format!("account '{account_id}' created and authenticated — launching."),
        );
        Ok(ResolvedAccount {
            id: account_id,
            source: AccountResolutionSource::Interactive,
        })
    }
}

enum AccountSelection {
    Existing(AccountId),
    AddNew,
}

fn prompt_select_account(accounts: &[AccountEntry]) -> Result<AccountSelection, AppError> {
    let mut options: Vec<String> = accounts
        .iter()
        .map(|a| {
            let auth_status = if a.has_auth {
                "authenticated"
            } else {
                "no auth!"
            };
            format!("{} ({auth_status})", a.id)
        })
        .collect();
    options.push("Add a new account".to_owned());

    let choice = inquire::Select::new("Select an account:", options.clone())
        .prompt()
        .map_err(|err| AccountError::NonInteractive {
            action: format!("account selection prompt: {err}"),
        })?;

    if choice == "Add a new account" {
        return Ok(AccountSelection::AddNew);
    }

    let index = options.iter().position(|o| o == &choice).unwrap_or(0);
    Ok(AccountSelection::Existing(accounts[index].id.clone()))
}

fn prompt_account_name() -> Result<AccountId, AppError> {
    let raw = inquire::Text::new("Account name:")
        .with_help_message("lowercase alphanumeric, hyphens, underscores (1-32 chars)")
        .prompt()
        .map_err(|err| AccountError::NonInteractive {
            action: format!("account name prompt: {err}"),
        })?;
    let id = raw
        .trim()
        .parse::<AccountId>()
        .map_err(|reason| AccountError::InvalidName { value: raw, reason })?;
    Ok(id)
}

fn do_add_account(ctx: &AppContext, account_id: &AccountId) -> Result<(), AppError> {
    let registry = Registry::from_config(&ctx.config);
    registry.add(account_id)?;

    narrate(
        ctx,
        &format!(
            "starting codex login — please authenticate in the browser \
            to link account '{account_id}'...",
        ),
    );
    let (_dir, auth_path) = match crate::commands::account::run_isolated_login(ctx) {
        Ok(result) => result,
        Err(err) => {
            let _ = registry.remove(account_id).inspect_err(|cleanup_err| {
                tracing::warn!(
                    op = "gate.add",
                    outcome = "cleanup-failed",
                    account = %account_id,
                    error = %cleanup_err
                );
            });
            return Err(err);
        }
    };

    narrate(ctx, "login succeeded — saving authentication token...");
    crate::commands::account::persist_auth_to_seed(&auth_path, &registry, account_id)?;
    registry.set_current(account_id)?;
    Ok(())
}

fn do_refresh_auth(ctx: &AppContext, account: &AccountId) -> Result<(), AppError> {
    narrate(
        ctx,
        &format!(
            "starting codex login — please authenticate in the browser \
            to renew the token for account '{account}'...",
        ),
    );
    let (_dir, auth_path) = crate::commands::account::run_isolated_login(ctx)?;

    narrate(
        ctx,
        "login succeeded — saving renewed authentication token...",
    );
    let registry = Registry::from_config(&ctx.config);
    crate::commands::account::persist_auth_to_seed(&auth_path, &registry, account)?;
    narrate(
        ctx,
        "clearing stale group auth tokens so new sessions use the fresh token...",
    );
    registry.delete_group_auths(account)?;
    Ok(())
}

/// Verify the token is valid server-side via a minimal `codex exec` probe.
///
/// Extracts the `[profiles.ping]` section from the user's composed
/// codex-session settings and serializes a minimal config.toml containing
/// only that section, suitable for writing to the probe's isolated
/// `CODEX_HOME`.
fn extract_ping_config(ctx: &AppContext) -> Result<String, AppError> {
    let profile_name = ctx.config.profile.active.as_deref().ok_or_else(|| {
        AppError::Account(AccountError::PingProfileMissing {
            detail: "no active codex-session profile".to_owned(),
        })
    })?;

    let composition = crate::services::profile::compose(
        profile_name,
        &crate::services::profile::ProfilePaths {
            profiles_dir: ctx.config.profile.profiles_dir.clone(),
            settings_dir: ctx.config.profile.settings_dir.clone(),
            cache_settings: cache_settings_path(ctx),
        },
    )?;

    let ping_table = composition
        .merged_config
        .get("profiles")
        .and_then(|v| v.as_table())
        .and_then(|profiles| profiles.get(PING_PROFILE))
        .and_then(|v| v.as_table())
        .ok_or_else(|| {
            let detail = format!(
                "[profiles.{PING_PROFILE}] not found in \
                composed settings for profile `{profile_name}`"
            );
            AppError::Account(AccountError::PingProfileMissing { detail })
        })?;

    let mut config = toml::Table::new();
    let mut profiles = toml::Table::new();
    profiles.insert(
        PING_PROFILE.to_owned(),
        toml::Value::Table(ping_table.clone()),
    );
    config.insert("profiles".to_owned(), toml::Value::Table(profiles));

    toml::to_string_pretty(&config).map_err(|err| AppError::Other(anyhow::anyhow!("{err}")))
}

fn cache_settings_path(ctx: &AppContext) -> Option<Utf8PathBuf> {
    let path = ctx.config.paths.cache_dir.join("settings.toml");
    path.is_file().then_some(path)
}

pub(crate) fn validate_ping_profile(ctx: &AppContext) -> Result<(), AppError> {
    extract_ping_config(ctx).map(|_| ())
}

/// Heartbeat probe — runs `codex --profile ping exec --json "say ok"` in
/// an isolated `CODEX_HOME` to verify the account's token works server-side.
///
/// The model is resolved by codex via `[profiles.ping]` — codex-session
/// never inspects or passes a model value. Users control the probe model
/// by setting `[profiles.ping].model` in their settings layer.
///
/// Returns `(valid, detail)`: `valid` is `Some(true)` when the token works,
/// `Some(false)` on a 401, and `None` on non-auth failures. `detail`
/// contains the combined stdout+stderr for the caller to display.
fn heartbeat_probe(
    ctx: &AppContext,
    account: &AccountId,
) -> Result<(Option<bool>, String), AppError> {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    const PROBE_TIMEOUT: Duration = Duration::from_secs(15);

    let ping_config = extract_ping_config(ctx)?;

    let registry = Registry::from_config(&ctx.config);
    let seed = registry.group_auth_seed_path(account);
    let bytes = crate::services::auth::secure_file_read(&seed)?;

    // Place the probe dir under state_dir, not /tmp — codex refuses to
    // create helper binaries when CODEX_HOME is under a temporary directory.
    let probe_parent = ctx.config.paths.state_dir.join("probe");
    std::fs::create_dir_all(probe_parent.as_std_path()).map_err(|source| {
        crate::services::auth::AuthError::Io {
            path: probe_parent.clone(),
            source,
        }
    })?;
    let tmp = tempfile::tempdir_in(probe_parent.as_std_path()).map_err(|source| {
        crate::services::auth::AuthError::Io {
            path: probe_parent,
            source,
        }
    })?;
    let tmp_path = camino::Utf8PathBuf::try_from(tmp.path().to_path_buf())
        .map_err(|err| AppError::Other(anyhow::anyhow!("{err}")))?;
    crate::adapters::fs::atomic_write(&tmp_path.join("auth.json"), &bytes)
        .map_err(crate::services::auth::AuthError::from)?;
    crate::adapters::fs::atomic_write(&tmp_path.join("config.toml"), ping_config.as_bytes())
        .map_err(crate::services::auth::AuthError::from)?;

    let binary = ctx
        .resolved_child()
        .map_err(crate::commands::account::map_spawner_error)?;

    let mut child = Command::new(binary.as_std_path())
        .args(["--profile", PING_PROFILE, "exec", "--json", "say ok"])
        .env_clear()
        .env("CODEX_HOME", tmp_path.as_str())
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", std::env::var("HOME").unwrap_or_default())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(AppError::ChildExec)?;

    let start = Instant::now();
    let status = loop {
        match child.try_wait().map_err(AppError::ChildExec)? {
            Some(status) => break status,
            None if start.elapsed() >= PROBE_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                let output = drain_child_output(&mut child);
                return Ok((
                    None,
                    format!("heartbeat probe timed out after {PROBE_TIMEOUT:?}\n{output}"),
                ));
            }
            None => std::thread::sleep(Duration::from_millis(200)),
        }
    };

    let raw_output = drain_child_output(&mut child);

    if status.success() {
        return Ok((Some(true), strip_codex_stdin_noise(&raw_output)));
    }
    if raw_output.contains("401") || raw_output.contains("Unauthorized") {
        return Ok((Some(false), raw_output));
    }
    if raw_output.contains("model_not_found")
        || raw_output.contains("decommissioned")
        || (raw_output.contains("model") && raw_output.contains("does not exist"))
    {
        let truncated = &raw_output[..raw_output.len().min(500)];
        return Ok((
            None,
            format!(
                "ping profile model may be deprecated or \
                unavailable — update [profiles.ping].model \
                in your codex settings layer.\n\
                API said: {truncated}"
            ),
        ));
    }
    Ok((None, raw_output))
}

fn drain_child_output(child: &mut std::process::Child) -> String {
    use std::io::Read as _;
    let mut combined = Vec::new();
    if let Some(mut pipe) = child.stdout.take() {
        let _ = pipe.read_to_end(&mut combined);
    }
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_end(&mut combined);
    }
    String::from_utf8_lossy(&combined).trim().to_owned()
}

/// Codex prints "Reading additional input from stdin..." to stderr when
/// stdin is /dev/null (not a TTY). Strip it only on success — on failure
/// the full output is diagnostic and should be preserved verbatim.
fn strip_codex_stdin_noise(output: &str) -> String {
    output
        .lines()
        .filter(|line| *line != "Reading additional input from stdin...")
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

pub(crate) fn probe_token(
    ctx: &AppContext,
    account: &AccountId,
) -> Result<(Option<bool>, String), AppError> {
    heartbeat_probe(ctx, account)
}

pub(crate) fn run_login(ctx: &AppContext, opts: &LoginOptions) -> Result<i32, AppError> {
    let state = assess(ctx)?;
    let interactive = std::io::stdin().is_terminal();

    match state {
        AccountState::NoAccounts => {
            if !interactive {
                return Err(AccountError::NoAccounts.into());
            }
            narrate(
                ctx,
                "no accounts registered. Setting up your first account for login.",
            );
            let account_id = prompt_account_name()?;
            narrate(
                ctx,
                &format!("creating account '{account_id}' and starting authentication..."),
            );
            do_add_account(ctx, &account_id)?;
            narrate(
                ctx,
                &format!("account '{account_id}' is now authenticated."),
            );
            Ok(0)
        }
        AccountState::NoneSelected { accounts } => {
            if !interactive {
                return Err(AccountError::NoneSelected.into());
            }
            narrate(
                ctx,
                &format!(
                    "{} account(s) found but none is selected. Please choose one to log in.",
                    accounts.len(),
                ),
            );
            login_resolve_none_selected(ctx, &accounts)?;
            Ok(0)
        }
        AccountState::AuthMissing { account } => {
            narrate(ctx, &format!("re-authenticating account '{account}'..."));
            do_refresh_auth(ctx, &account)?;
            narrate(ctx, &format!("account '{account}' is now authenticated."));
            Ok(0)
        }
        AccountState::Ready(resolved) => login_handle_ready(ctx, opts, &resolved),
    }
}

fn login_handle_ready(
    ctx: &AppContext,
    opts: &LoginOptions,
    resolved: &ResolvedAccount,
) -> Result<i32, AppError> {
    if opts.force {
        narrate(
            ctx,
            &format!(
                "force-refreshing authentication for account '{}' (source: {})...",
                resolved.id,
                source_label(resolved.source),
            ),
        );
        do_refresh_auth(ctx, &resolved.id)?;
        narrate(
            ctx,
            &format!("account '{}' is now authenticated.", resolved.id),
        );
        return Ok(0);
    }

    narrate(
        ctx,
        &format!(
            "verifying token for account '{}' (source: {})...",
            resolved.id,
            source_label(resolved.source),
        ),
    );
    let emit_stderr = |stderr: &str| {
        if !stderr.is_empty() {
            let _ = ctx.ui.write_warning(stderr);
        }
    };

    match heartbeat_probe(ctx, &resolved.id) {
        Ok((Some(true), stderr)) => {
            emit_stderr(&stderr);
            narrate(
                ctx,
                &format!("account '{}' is already authenticated.", resolved.id),
            );
            Ok(0)
        }
        Ok((Some(false), stderr)) => {
            emit_stderr(&stderr);
            narrate(
                ctx,
                &format!(
                    "account '{}' token is invalid — re-authenticating...",
                    resolved.id,
                ),
            );
            do_refresh_auth(ctx, &resolved.id)?;
            narrate(
                ctx,
                &format!("account '{}' is now authenticated.", resolved.id),
            );
            Ok(0)
        }
        Ok((None, stderr)) => {
            // Probe failed for a non-auth reason (timeout, binary issue, env).
            // This says nothing about token validity — assume the token is fine.
            // The user can run `login --force` if they know it's actually broken.
            emit_stderr(&stderr);
            narrate(
                ctx,
                &format!(
                    "account '{}' is already authenticated. \
                    Use `login --force` if you need to re-authenticate.",
                    resolved.id,
                ),
            );
            Ok(0)
        }
        Err(err) => {
            // Probe could not even start (binary missing, seed unreadable).
            narrate(
                ctx,
                &format!(
                    "account '{}' is already authenticated ({err}). \
                    Use `login --force` if you need to re-authenticate.",
                    resolved.id,
                ),
            );
            Ok(0)
        }
    }
}

pub(crate) fn run_logout(ctx: &AppContext) -> Result<i32, AppError> {
    let state = assess(ctx)?;
    let interactive = std::io::stdin().is_terminal();

    match state {
        AccountState::NoAccounts => {
            narrate(ctx, "no accounts registered — nothing to log out.");
            Ok(0)
        }
        AccountState::NoneSelected { accounts } => {
            if !interactive {
                return Err(AccountError::NoneSelected.into());
            }
            narrate(
                ctx,
                &format!(
                    "{} account(s) found but none is selected. Please choose one to log out.",
                    accounts.len(),
                ),
            );
            let account_id = logout_resolve_none_selected(&accounts)?;
            do_logout(ctx, &account_id)?;
            narrate(ctx, &format!("account '{account_id}' is now logged out."));
            Ok(0)
        }
        AccountState::AuthMissing { account } => {
            narrate(
                ctx,
                &format!("account '{account}' has no valid authentication — already logged out."),
            );
            let registry = Registry::from_config(&ctx.config);
            let _ = registry.delete_auth_seed(&account);
            Ok(0)
        }
        AccountState::Ready(resolved) => {
            narrate(
                ctx,
                &format!(
                    "logging out account '{}' (source: {})...",
                    resolved.id,
                    source_label(resolved.source),
                ),
            );
            do_logout(ctx, &resolved.id)?;
            narrate(
                ctx,
                &format!("account '{}' is now logged out.", resolved.id),
            );
            Ok(0)
        }
    }
}

fn login_resolve_none_selected(
    ctx: &AppContext,
    accounts: &[AccountEntry],
) -> Result<AccountId, AppError> {
    let selected = prompt_select_account(accounts)?;
    match selected {
        AccountSelection::Existing(account_id) => {
            let registry = Registry::from_config(&ctx.config);
            registry.set_current(&account_id)?;
            narrate(
                ctx,
                &format!("selected account '{account_id}' — authenticating..."),
            );
            do_refresh_auth(ctx, &account_id)?;
            narrate(
                ctx,
                &format!("account '{account_id}' is now authenticated."),
            );
            Ok(account_id)
        }
        AccountSelection::AddNew => {
            let account_id = prompt_account_name()?;
            narrate(
                ctx,
                &format!("creating account '{account_id}' and starting authentication..."),
            );
            do_add_account(ctx, &account_id)?;
            narrate(
                ctx,
                &format!("account '{account_id}' is now authenticated."),
            );
            Ok(account_id)
        }
    }
}

fn logout_resolve_none_selected(accounts: &[AccountEntry]) -> Result<AccountId, AppError> {
    let options: Vec<String> = accounts
        .iter()
        .map(|a| {
            let auth_status = if a.has_auth {
                "authenticated"
            } else {
                "no auth"
            };
            format!("{} ({auth_status})", a.id)
        })
        .collect();

    let choice = inquire::Select::new("Select an account to log out:", options.clone())
        .prompt()
        .map_err(|err| AccountError::NonInteractive {
            action: format!("logout account selection: {err}"),
        })?;

    let index = options.iter().position(|o| o == &choice).unwrap_or(0);
    Ok(accounts[index].id.clone())
}

fn do_logout(ctx: &AppContext, account: &AccountId) -> Result<(), AppError> {
    let registry = Registry::from_config(&ctx.config);
    let seed = registry.group_auth_seed_path(account);

    // Copy the account's token into an isolated CODEX_HOME so codex
    // finds and revokes the correct token — not whatever is in ~/.codex/.
    if seed.as_std_path().exists() {
        narrate(ctx, "revoking token via isolated codex logout...");
        match revoke_via_isolated_logout(ctx, &seed) {
            Ok(()) => {}
            Err(err) => {
                tracing::warn!(
                    op = "gate.logout",
                    outcome = "isolated-logout-failed-non-fatal",
                    account = %account,
                    error = %err
                );
            }
        }
    }

    registry.delete_auth_seed(account)?;
    registry.delete_group_auths(account)?;
    Ok(())
}

fn revoke_via_isolated_logout(
    ctx: &AppContext,
    seed_path: &camino::Utf8Path,
) -> Result<(), AppError> {
    let dir = crate::commands::account::create_auth_ops_dir(ctx)?;
    let home = camino::Utf8PathBuf::try_from(dir.path().to_path_buf())
        .map_err(|err| AppError::Other(anyhow::anyhow!("{err}")))?;
    let bytes = crate::services::auth::secure_file_read(seed_path)?;
    crate::adapters::fs::atomic_write(&home.join("auth.json"), &bytes)
        .map_err(crate::services::auth::AuthError::from)?;
    crate::commands::account::spawn_child_isolated(ctx, ["logout"], Some(&home))
}

fn narrate(ctx: &AppContext, msg: &str) {
    if ctx.global.silent {
        return;
    }
    let _ = ctx.ui.write_prompt(&format!("[codex-session] {msg}\n"));
}
