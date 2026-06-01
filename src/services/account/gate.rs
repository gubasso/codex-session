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
    resolver::{AccountIntent, AccountResolutionSource, ResolvedAccount, source_label},
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
    ReadyPinned(ResolvedAccount),
    ReadyAuto {
        candidates: usize,
    },
    AuthMissing {
        account: AccountId,
    },
    PinnedNotFound {
        account: AccountId,
    },
    #[allow(dead_code)]
    NoneSelected {
        accounts: Vec<AccountEntry>,
    },
    NoAccounts,
}

pub(crate) fn assess(ctx: &AppContext) -> Result<AccountState, AppError> {
    let registry = Registry::from_config(&ctx.config);
    let accounts = registry.list()?;

    match super::resolver::intent(ctx)? {
        AccountIntent::Pinned { id, source } => match registry.expect_account_dir(&id) {
            Err(AccountError::NotFound { .. }) => Ok(AccountState::PinnedNotFound { account: id }),
            Err(err) => Err(err.into()),
            Ok(_) => {
                let seed_path = registry.group_auth_seed_path(&id);
                if !seed_path.as_std_path().exists() {
                    return Ok(AccountState::AuthMissing { account: id });
                }
                Ok(AccountState::ReadyPinned(ResolvedAccount { id, source }))
            }
        },
        AccountIntent::Auto => {
            if accounts.is_empty() {
                return Ok(AccountState::NoAccounts);
            }
            let candidates = accounts
                .iter()
                .filter(|entry| super::selector::is_usable(ctx, &registry, entry))
                .count();
            Ok(AccountState::ReadyAuto { candidates })
        }
    }
}

pub(crate) enum GateOutcome {
    Resolved(ResolvedAccount),
    AutoDeferred,
}

impl GateOutcome {
    pub(crate) const fn resolved_or_none(&self) -> Option<&ResolvedAccount> {
        match self {
            Self::Resolved(resolved) => Some(resolved),
            Self::AutoDeferred => None,
        }
    }
}

pub(crate) fn ensure(ctx: &AppContext) -> Result<GateOutcome, AppError> {
    let state = assess(ctx)?;
    let interactive = std::io::stdin().is_terminal();

    match state {
        AccountState::ReadyPinned(resolved) => {
            narrate(
                ctx,
                &format!(
                    "account '{}' (source: {}), auth present — launching.",
                    resolved.id,
                    source_label(resolved.source),
                ),
            );
            Ok(GateOutcome::Resolved(resolved))
        }
        AccountState::ReadyAuto { candidates } => {
            narrate(
                ctx,
                &format!("auto-selection enabled ({candidates} candidate(s))."),
            );
            Ok(GateOutcome::AutoDeferred)
        }
        AccountState::NoAccounts => {
            if !interactive {
                return Err(AccountError::NoAccounts.into());
            }
            narrate(
                ctx,
                "no accounts registered. Let's set up your first account.",
            );
            interactive_resolve_no_accounts(ctx).map(GateOutcome::Resolved)
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
            interactive_resolve_none_selected(ctx, &accounts).map(GateOutcome::Resolved)
        }
        AccountState::AuthMissing { account, .. } => {
            if !interactive {
                return Err(AccountError::AuthMissing { name: account }.into());
            }
            warn_and_confirm_auth_missing(ctx, &account).map(GateOutcome::Resolved)
        }
        AccountState::PinnedNotFound { account } => Err(pinned_not_found_error(ctx, &account)),
    }
}

fn pinned_not_found_error(ctx: &AppContext, account: &AccountId) -> AppError {
    let registry = Registry::from_config(&ctx.config);
    AccountError::NotFound {
        name: account.clone(),
        path: registry.account_dir(account),
    }
    .into()
}

fn auto_resolved_without_pick(ctx: &AppContext) -> Result<Option<ResolvedAccount>, AppError> {
    let registry = Registry::from_config(&ctx.config);
    let entries = registry.list()?;
    // Side-effectful auth ops (login/logout) must act on the user-visible active
    // account — the last committed selection shown by `account current` — not an
    // arbitrary alphabetically-first usable entry. Prefer `current()` when it is
    // still usable; only fall back to the first usable entry when no current is
    // set. Never call `selector::pick` here (no quota fetch, no `set_current`).
    if let Some(current) = registry.current()?
        && let Some(entry) = entries.iter().find(|e| e.id == current)
        && super::selector::is_usable(ctx, &registry, entry)
    {
        return Ok(Some(ResolvedAccount {
            id: current,
            source: AccountResolutionSource::Auto,
        }));
    }
    for entry in entries {
        if super::selector::is_usable(ctx, &registry, &entry) {
            return Ok(Some(ResolvedAccount {
                id: entry.id,
                source: AccountResolutionSource::Auto,
            }));
        }
    }
    Ok(None)
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

/// Returns the raw TOML bytes of the active config-recipe's `ping` profile
/// file (`profiles/ping.config.toml`). The bytes are written
/// verbatim to `$CODEX_HOME/ping.config.toml` under the probe's isolated
/// `CODEX_HOME` — codex v0.134+ requires per-profile overrides to live in
/// sibling files with bare top-level keys.
fn extract_ping_config(ctx: &AppContext) -> Result<String, AppError> {
    let recipe_name = ctx.config.config_recipe.active.as_deref().ok_or_else(|| {
        AppError::Account(AccountError::PingProfileMissing {
            detail: "no active codex-session config-recipe".to_owned(),
        })
    })?;

    let composition = crate::services::config_recipe::compose(
        recipe_name,
        &crate::services::config_recipe::ConfigRecipePaths {
            recipes_dir: ctx.config.config_recipe.recipes_dir.clone(),
            configs_dir: ctx.config.config_recipe.configs_dir.clone(),
            profiles_dir: ctx.config.config_recipe.profiles_dir.clone(),
            cache_config: cache_config_path(ctx),
        },
    )?;

    let ping = composition
        .profile_files
        .iter()
        .find(|p| p.name == PING_PROFILE)
        .ok_or_else(|| {
            let detail = format!(
                "profile file `profiles/{PING_PROFILE}.config.toml` \
                not found in active config-recipe `{recipe_name}` \
                (manifest `profile-files` list may exclude it)"
            );
            AppError::Account(AccountError::PingProfileMissing { detail })
        })?;

    Ok(ping.raw_toml.clone())
}

fn cache_config_path(ctx: &AppContext) -> Option<Utf8PathBuf> {
    let path = ctx.config.paths.cache_dir.join("configs.toml");
    path.is_file().then_some(path)
}

pub(crate) fn validate_ping_config_recipe(ctx: &AppContext) -> Result<(), AppError> {
    extract_ping_config(ctx).map(|_| ())
}

/// Heartbeat probe — runs `codex --profile ping exec --json "say ok"` in
/// an isolated `CODEX_HOME` to verify the account's token works server-side.
///
/// The model is resolved by codex via the sibling
/// `$CODEX_HOME/ping.config.toml`, which is the verbatim bytes of the
/// active config-recipe's `profiles/ping.config.toml`. Users
/// control the probe model by editing that file.
///
/// Returns `(valid, detail)`: `valid` is `Some(true)` when the token works,
/// `Some(false)` on a 401, and `None` on non-auth failures. `detail`
/// contains the combined stdout+stderr for the caller to display.
async fn heartbeat_probe(
    ctx: &AppContext,
    auth_source: &camino::Utf8Path,
) -> Result<(Option<bool>, String), AppError> {
    use std::process::Stdio;
    use std::time::Duration;
    use tokio::process::Command;

    const PROBE_TIMEOUT: Duration = Duration::from_secs(15);

    ctx.ensure_child_version()?;
    let ping_config = extract_ping_config(ctx)?;

    let bytes = crate::services::auth::secure_file_read(auth_source)?;

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
    let tmp_auth = tmp_path.join("auth.json");
    crate::adapters::fs::atomic_write(&tmp_auth, &bytes)
        .map_err(crate::services::auth::AuthError::from)?;
    // Empty base config.toml — codex requires the file to exist; all overrides
    // come from the sibling profile file written below.
    crate::adapters::fs::atomic_write(&tmp_path.join("config.toml"), b"")
        .map_err(crate::services::auth::AuthError::from)?;
    crate::adapters::fs::atomic_write(
        &tmp_path.join(format!("{PING_PROFILE}.config.toml")),
        ping_config.as_bytes(),
    )
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
        .kill_on_drop(true)
        .spawn()
        .map_err(AppError::ChildExec)?;

    let status = match tokio::time::timeout(PROBE_TIMEOUT, child.wait()).await {
        Ok(Ok(status)) => status,
        Ok(Err(err)) => return Err(AppError::ChildExec(err)),
        Err(_elapsed) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            let output = drain_child_output(&mut child).await;
            persist_probe_rotation_or_warn(auth_source, &bytes, &tmp_auth);
            return Ok((
                None,
                format!("heartbeat probe timed out after {PROBE_TIMEOUT:?}\n{output}"),
            ));
        }
    };

    let raw_output = drain_child_output(&mut child).await;
    persist_probe_rotation_or_warn(auth_source, &bytes, &tmp_auth);

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
                unavailable — update profiles/{PING_PROFILE}.config.toml \
                in your codex-session config tree.\n\
                API said: {truncated}"
            ),
        ));
    }
    Ok((None, raw_output))
}

fn persist_probe_rotation_or_warn(
    auth_source: &camino::Utf8Path,
    original: &[u8],
    tmp_auth: &camino::Utf8Path,
) {
    if let Err(err) = persist_probe_rotation(auth_source, original, tmp_auth) {
        tracing::warn!(
            op = "probe.persist_rotation",
            path = %auth_source,
            error = %err,
            "failed to persist probe auth rotation"
        );
    }
}

fn auth_bytes_access_token(bytes: &[u8]) -> Option<String> {
    let auth: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    auth.get("tokens")
        .and_then(|tokens| tokens.get("access_token"))
        .and_then(serde_json::Value::as_str)
        .filter(|token| !token.is_empty())
        .map(ToOwned::to_owned)
}

fn persist_probe_rotation(
    auth_source: &camino::Utf8Path,
    original: &[u8],
    tmp_auth: &camino::Utf8Path,
) -> Result<(), AppError> {
    let updated = match std::fs::read(tmp_auth.as_std_path()) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(crate::services::auth::AuthError::Io {
                path: tmp_auth.to_path_buf(),
                source,
            }
            .into());
        }
    };

    if updated == original {
        return Ok(());
    }
    if auth_bytes_access_token(&updated).is_none() {
        tracing::warn!(
            op = "probe.persist_rotation",
            path = %auth_source,
            "probe auth changed but has no non-empty access_token; not persisting"
        );
        return Ok(());
    }

    crate::adapters::fs::atomic_write(auth_source, &updated)
        .map_err(crate::services::auth::AuthError::from)?;
    tracing::info!(
        op = "probe.persist_rotation",
        path = %auth_source,
        "persisted child-rotated auth back to source"
    );
    Ok(())
}

async fn drain_child_output(child: &mut tokio::process::Child) -> String {
    use tokio::io::AsyncReadExt as _;

    let mut combined = Vec::new();
    if let Some(mut pipe) = child.stdout.take() {
        let _ = pipe.read_to_end(&mut combined).await;
    }
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_end(&mut combined).await;
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

pub(crate) async fn probe_token(
    ctx: &AppContext,
    account: &AccountId,
) -> Result<(Option<bool>, String), AppError> {
    let registry = Registry::from_config(&ctx.config);
    let seed = registry.group_auth_seed_path(account);
    heartbeat_probe(ctx, &seed).await
}

/// Probe a specific auth file rather than the account seed.
///
/// Used by `account health` to re-probe against freshly-rotated credentials
/// after `quota::refresh` performs a 401 token rotation, so the probe never
/// races quota on `OpenAI`'s single-use refresh token.
pub(crate) async fn probe_token_with_auth(
    ctx: &AppContext,
    auth_source: &camino::Utf8Path,
) -> Result<(Option<bool>, String), AppError> {
    heartbeat_probe(ctx, auth_source).await
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
        AccountState::ReadyPinned(resolved) => login_handle_ready(ctx, opts, &resolved),
        AccountState::ReadyAuto { .. } => {
            if let Some(resolved) = auto_resolved_without_pick(ctx)? {
                login_handle_ready(ctx, opts, &resolved)
            } else if interactive {
                let registry = Registry::from_config(&ctx.config);
                let accounts = registry.list()?;
                narrate(
                    ctx,
                    &format!(
                        "{} account(s) found but none is ready. Please choose one to log in.",
                        accounts.len(),
                    ),
                );
                login_resolve_none_selected(ctx, &accounts)?;
                Ok(0)
            } else {
                Err(AccountError::NoneSelected.into())
            }
        }
        AccountState::PinnedNotFound { account } => Err(pinned_not_found_error(ctx, &account)),
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

    let seed = Registry::from_config(&ctx.config).group_auth_seed_path(&resolved.id);
    match crate::runtime::block_on(heartbeat_probe(ctx, &seed)) {
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
            // A child-version mismatch is a hard, user-actionable error —
            // propagate it as exit 78 so the user sees it directly rather
            // than getting a misleading "already authenticated" message.
            if matches!(err, AppError::ChildVersionTooOld { .. }) {
                return Err(err);
            }
            // Other probe failures (binary missing, seed unreadable) say
            // nothing about token validity — assume the token is fine and
            // surface the error inline.
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
        AccountState::ReadyPinned(resolved) => {
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
        AccountState::ReadyAuto { .. } => {
            if let Some(resolved) = auto_resolved_without_pick(ctx)? {
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
            } else if interactive {
                let registry = Registry::from_config(&ctx.config);
                let accounts = registry.list()?;
                narrate(
                    ctx,
                    &format!(
                        "{} account(s) found but none is ready. Please choose one to log out.",
                        accounts.len(),
                    ),
                );
                let account_id = logout_resolve_none_selected(&accounts)?;
                do_logout(ctx, &account_id)?;
                narrate(ctx, &format!("account '{account_id}' is now logged out."));
                Ok(0)
            } else {
                Err(AccountError::NoneSelected.into())
            }
        }
        AccountState::PinnedNotFound { account } => Err(pinned_not_found_error(ctx, &account)),
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
            // A child-version mismatch is a hard, user-actionable error —
            // surface it as exit 78 instead of silently deleting the seed.
            Err(err) if matches!(err, AppError::ChildVersionTooOld { .. }) => {
                return Err(err);
            }
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
    let use_color = crate::ui::color::stderr_color();
    let prefix = format!(
        "{}[codex-session]{}",
        crate::ui::style_open(crate::ui::DIM, use_color),
        crate::ui::style_close(crate::ui::DIM, use_color)
    );
    let _ = ctx.ui.write_prompt(&format!("{prefix} {msg}\n"));
}
