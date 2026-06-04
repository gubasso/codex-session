//! `doctor` command.
//!
//! What this is: full validation of the codex-session config setup.
//! What this is not: read-only introspection — that lives in
//! `commands::config_status` and is intentionally exit-0 even on
//! malformed config.
//!
//! Each check returns a `CheckResult`. Exit code is 0 when no check
//! reports FAIL (WARNs are allowed), 1 otherwise.

#![allow(clippy::missing_errors_doc, clippy::result_large_err)]

use std::collections::{BTreeMap, BTreeSet};
use std::time::SystemTime;

use camino::{Utf8Path, Utf8PathBuf};

use crate::ui::spinner::{SpinnerGroup, SpinnerHandle, should_show_spinner};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum CheckStatus {
    Ok,
    Warn,
    Fail,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct CheckResult {
    pub(crate) name: String,
    pub(crate) status: CheckStatus,
    pub(crate) detail: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct CheckGroup {
    pub(crate) name: String,
    pub(crate) checks: Vec<CheckResult>,
}

const GROUP_ENVIRONMENT: &str = "environment";
const GROUP_ACCOUNTS: &str = "accounts";
const GROUP_CONFIG_RECIPE: &str = "config-recipe";
const GROUP_SESSION: &str = "session";
const GROUP_AUTH: &str = "auth";
const GROUP_ONLINE: &str = "online";

#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct CheckSummary {
    pub(crate) ok: usize,
    pub(crate) warn: usize,
    pub(crate) fail: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct DoctorReport {
    pub(crate) config_recipe: Option<String>,
    pub(crate) account: String,
    pub(crate) account_source: String,
    pub(crate) group_id: String,
    pub(crate) group_id_source: String,
    pub(crate) codex_home: Utf8PathBuf,
    pub(crate) accounts: Vec<DoctorAccountEntry>,
    pub(crate) active_account: DoctorActiveAccount,
    pub(crate) groups: Vec<CheckGroup>,
    pub(crate) summary: CheckSummary,
    pub(crate) next_steps: Vec<String>,
    /// Per-config-recipe merged env, keyed by config-recipe name. Empty unless
    /// `--show-env` was passed. In single-config-recipe mode this contains at
    /// most one entry (the active config-recipe); in `--all-config-recipes` mode it
    /// can contain one entry per config-recipe whose composition succeeded.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) env: BTreeMap<String, BTreeMap<String, String>>,
}

impl DoctorReport {
    /// Iterate every check across all groups, in group order.
    pub(crate) fn all_checks(&self) -> impl Iterator<Item = &CheckResult> {
        self.groups.iter().flat_map(|group| group.checks.iter())
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct DoctorAccountEntry {
    pub(crate) name: String,
    pub(crate) has_auth: bool,
    pub(crate) last_used_at_unix: Option<u64>,
    pub(crate) current: bool,
    pub(crate) cooldown_active: bool,
    pub(crate) cooldown_reset_at_unix: Option<u64>,
    pub(crate) cooldown_reason: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct DoctorActiveAccount {
    pub(crate) name: String,
    pub(crate) source: String,
}

pub(crate) fn run(
    ctx: &crate::context::AppContext,
    args: crate::cli::doctor::DoctorArgs,
) -> Result<u8, crate::error::AppError> {
    let fmt = ctx.global.format.unwrap_or_default();
    let spinners = SpinnerGroup::new(should_show_spinner(ctx, fmt, false));
    let spinner = spinners.add("Running checks...");
    let report = build_report(ctx, args, Some(&spinner));
    spinner.finish_and_clear();
    ctx.ui.write_doctor(&report, fmt)?;
    Ok(u8::from(report.summary.fail > 0))
}

#[allow(clippy::too_many_lines)]
fn build_report(
    ctx: &crate::context::AppContext,
    args: crate::cli::doctor::DoctorArgs,
    progress: Option<&SpinnerHandle>,
) -> DoctorReport {
    let mut environment_checks: Vec<CheckResult> = Vec::new();
    let mut account_checks: Vec<CheckResult> = Vec::new();
    let mut config_recipe_checks: Vec<CheckResult> = Vec::new();
    let mut session_checks: Vec<CheckResult> = Vec::new();
    let mut auth_checks: Vec<CheckResult> = Vec::new();
    let mut online_checks: Vec<CheckResult> = Vec::new();
    let mut next_steps: Vec<String> = Vec::new();
    let mut env_dump: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    set_progress(progress, "Inspecting session root...");
    let root = crate::services::session::dir::inspect_session_root(
        ctx.config.paths.runtime_dir.as_deref(),
        &ctx.config.paths.state_dir,
    )
    .ok();
    set_progress(progress, "Resolving account...");
    let display_account_result = crate::services::account::resolver::resolve_for_display(ctx);
    if let Err(err) = &display_account_result {
        session_checks.push(fail("session.account", err.to_string()));
    }
    let (resolved_account, account_name, account_source) = display_account_result.map_or_else(
        |_| (None, "(unresolved)".to_owned(), "error".to_owned()),
        |display| match display {
            crate::services::account::resolver::DisplayAccount::Pinned { id, source } => {
                let name = id.to_string();
                (
                    Some(crate::services::account::resolver::ResolvedAccount { id, source }),
                    name,
                    crate::services::account::resolver::source_label(source).to_owned(),
                )
            }
            crate::services::account::resolver::DisplayAccount::Auto {
                last_selected: Some(id),
            } => {
                let name = id.to_string();
                (
                    Some(crate::services::account::resolver::ResolvedAccount {
                        id,
                        source: crate::services::account::resolver::AccountResolutionSource::Auto,
                    }),
                    name,
                    "auto".to_owned(),
                )
            }
            crate::services::account::resolver::DisplayAccount::Auto {
                last_selected: None,
            } => (
                None,
                "(auto — none selected yet)".to_owned(),
                "auto".to_owned(),
            ),
        },
    );
    set_progress(progress, "Resolving session group...");
    let (group_id, group_id_source, codex_home) = match (
        crate::services::session::group_id::current(ctx),
        root.as_ref(),
        resolved_account.as_ref(),
    ) {
        (Ok(resolved_group), Some(inspected_root), Some(account)) => {
            let codex_home = crate::services::session::dir::inspect_session_dir(
                &inspected_root.root.path,
                &account.id,
                resolved_group.id.as_str(),
            )
            .map(|dir| dir.path)
            .unwrap_or_default();
            (
                resolved_group.id.as_str().to_owned(),
                group_id_source_label(resolved_group.source).to_owned(),
                codex_home,
            )
        }
        (Ok(resolved_group), _, _) => (
            resolved_group.id.as_str().to_owned(),
            group_id_source_label(resolved_group.source).to_owned(),
            Utf8PathBuf::new(),
        ),
        (Err(err), _, _) => {
            session_checks.push(fail("session.group_id", err.to_string()));
            (
                "(unresolved)".to_owned(),
                "error".to_owned(),
                Utf8PathBuf::new(),
            )
        }
    };
    set_progress(progress, "Inspecting accounts...");
    let registry = crate::services::account::registry::Registry::from_config(&ctx.config);
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    // Surface registry I/O errors as a `fail` check rather than silently
    // hiding them behind `unwrap_or_default()`.
    let accounts = match registry.list() {
        Ok(entries) => {
            let mut accs = Vec::with_capacity(entries.len());
            for entry in entries {
                let cd = match crate::services::account::cooldown::read(&entry.dir) {
                    Ok(cd) => cd,
                    Err(err) => {
                        account_checks.push(warn(
                            format!("account.{}.cooldown.read", entry.id),
                            err.to_string(),
                        ));
                        None
                    }
                };
                accs.push(DoctorAccountEntry {
                    current: resolved_account
                        .as_ref()
                        .is_some_and(|active| active.id == entry.id),
                    has_auth: entry.has_auth,
                    last_used_at_unix: entry
                        .last_used_at
                        .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|value| value.as_secs()),
                    name: entry.id.to_string(),
                    cooldown_active: cd.as_ref().is_some_and(|c| {
                        crate::services::account::cooldown::is_active(c, now_unix)
                    }),
                    cooldown_reset_at_unix: cd.as_ref().map(|c| c.reset_at_unix),
                    cooldown_reason: cd.map(|c| c.reason),
                });
            }
            accs
        }
        Err(err) => {
            account_checks.push(fail("accounts.registry", err.to_string()));
            Vec::new()
        }
    };
    let active_name = resolved_account.as_ref().map(|a| a.id.as_str());
    account_checks.extend(check_account_health(&accounts, active_name));

    // 1. Active config-recipe resolution + source.
    set_progress(progress, "Checking active config recipe...");
    let active = ctx.config.config_recipe.active.clone();
    config_recipe_checks.push(check_active_config_recipe(ctx, active.as_deref()));

    // 2-8. Per-config-recipe checks: manifest, layers, composition, env extraction.
    set_progress(progress, "Checking config recipes...");
    if args.all_config_recipes {
        match discover_recipes(&ctx.config.config_recipe.recipes_dir) {
            DiscoveredRecipes::Found(recipes) if recipes.is_empty() => {
                config_recipe_checks.push(warn(
                    "config-recipes.all",
                    format!(
                        "no config-recipe manifests found in {}",
                        ctx.config.config_recipe.recipes_dir
                    ),
                ));
                next_steps.push(format!(
                    "create a manifest at {}/<name>.yaml",
                    ctx.config.config_recipe.recipes_dir
                ));
            }
            DiscoveredRecipes::Found(recipes) => {
                for name in &recipes {
                    check_one_recipe(
                        ctx,
                        name,
                        &mut config_recipe_checks,
                        &mut env_dump,
                        args.show_env,
                    );
                }
            }
            DiscoveredRecipes::NotFound => {
                config_recipe_checks.push(warn(
                    "config-recipes.all",
                    format!(
                        "config-recipes directory does not exist: {}",
                        ctx.config.config_recipe.recipes_dir
                    ),
                ));
                next_steps.push(format!(
                    "create a manifest at {}/<name>.yaml",
                    ctx.config.config_recipe.recipes_dir
                ));
            }
            DiscoveredRecipes::Unreadable(reason) => {
                config_recipe_checks.push(fail(
                    "config-recipes.all",
                    format!(
                        "could not read config-recipes directory {}: {reason}",
                        ctx.config.config_recipe.recipes_dir
                    ),
                ));
            }
        }
    } else if let Some(name) = active.as_deref() {
        check_one_recipe(
            ctx,
            name,
            &mut config_recipe_checks,
            &mut env_dump,
            args.show_env,
        );
    } else {
        next_steps.push(format!(
            "create a manifest at {}/default.yaml or pass --config-recipe NAME",
            ctx.config.config_recipe.recipes_dir
        ));
    }
    config_recipe_checks.push(check_ping_config(ctx));

    // 9. Orphan config layers.
    set_progress(progress, "Checking config layers...");
    config_recipe_checks.push(check_orphan_layers(ctx));
    set_progress(progress, "Checking legacy profile forms...");
    config_recipe_checks.extend(check_legacy_profile_forms(ctx));

    // 11. XDG paths.
    set_progress(progress, "Checking XDG paths...");
    environment_checks.push(check_xdg_paths());

    // 12. Session root.
    set_progress(progress, "Checking session root...");
    session_checks.push(check_session_root(ctx));
    set_progress(progress, "Checking trust cache...");
    session_checks.push(check_trust_cache(ctx));
    set_progress(progress, "Checking session permissions...");
    session_checks.push(check_session_permissions(ctx));

    // 13. Child binary.
    set_progress(progress, "Checking codex binary...");
    environment_checks.push(check_child_binary(ctx));
    set_progress(progress, "Checking codex version...");
    environment_checks.push(check_codex_version_minimum(ctx));

    // 14. Session sidecar inventory.
    set_progress(progress, "Checking session inventory...");
    session_checks.push(check_session_inventory(ctx));

    // 15. Native auth bridge health.
    set_progress(progress, "Checking native auth...");
    auth_checks.push(check_auth_native(ctx));

    if args.online
        && let Some(account) = resolved_account.as_ref()
    {
        set_progress(progress, "Probing token + quota (parallel)...");
        let [token_check, quota_check] =
            crate::runtime::block_on(run_online_checks(ctx, &account.id));
        online_checks.push(token_check);
        online_checks.push(quota_check);
    }

    set_progress(progress, "Summarizing checks...");
    let mut groups = Vec::new();
    push_group(&mut groups, GROUP_ENVIRONMENT, environment_checks);
    push_group(&mut groups, GROUP_ACCOUNTS, account_checks);
    push_group(&mut groups, GROUP_CONFIG_RECIPE, config_recipe_checks);
    push_group(&mut groups, GROUP_SESSION, session_checks);
    push_group(&mut groups, GROUP_AUTH, auth_checks);
    push_group(&mut groups, GROUP_ONLINE, online_checks);

    let mut report = DoctorReport {
        config_recipe: active,
        account: account_name.clone(),
        account_source: account_source.clone(),
        group_id,
        group_id_source,
        codex_home,
        accounts,
        active_account: DoctorActiveAccount {
            name: account_name,
            source: account_source,
        },
        groups,
        summary: CheckSummary::default(),
        next_steps: Vec::new(),
        env: env_dump,
    };

    let mut collected_next_steps = next_steps;
    populate_next_steps(report.all_checks(), &mut collected_next_steps);
    report.next_steps = collected_next_steps;
    report.summary = summarize(report.all_checks());
    report
}

fn push_group(groups: &mut Vec<CheckGroup>, name: &str, checks: Vec<CheckResult>) {
    if !checks.is_empty() {
        groups.push(CheckGroup {
            name: name.to_owned(),
            checks,
        });
    }
}

fn set_progress(progress: Option<&SpinnerHandle>, message: &'static str) {
    if let Some(progress) = progress {
        progress.set_message(message);
    }
}

fn check_one_recipe(
    ctx: &crate::context::AppContext,
    name: &str,
    checks: &mut Vec<CheckResult>,
    env_dump: &mut BTreeMap<String, BTreeMap<String, String>>,
    show_env: bool,
) {
    let manifest_path = ctx
        .config
        .config_recipe
        .recipes_dir
        .join(format!("{name}.yaml"));

    // 2. Manifest exists.
    let exists = match std::fs::metadata(manifest_path.as_std_path()) {
        Ok(meta) if meta.is_file() => true,
        Ok(_) => {
            checks.push(fail(
                format!("manifest.{name}.exists"),
                format!("{manifest_path} is not a file"),
            ));
            return;
        }
        Err(err) => {
            checks.push(fail(
                format!("manifest.{name}.exists"),
                format!("{manifest_path}: {err}"),
            ));
            return;
        }
    };
    if !exists {
        return;
    }
    checks.push(ok(
        format!("manifest.{name}.exists"),
        manifest_path.to_string(),
    ));

    // 3+4. Parse + schema (combined in Manifest::parse).
    let manifest = match crate::services::config_recipe::Manifest::parse(manifest_path) {
        Ok(m) => {
            checks.push(ok(
                format!("manifest.{name}.parse"),
                format!("{} layers", m.config_layers.len()),
            ));
            m
        }
        Err(err) => {
            checks.push(fail(format!("manifest.{name}.parse"), err.to_string()));
            return;
        }
    };

    // 5-7. Per-layer existence, parse, env validation.
    for layer_name in &manifest.config_layers {
        check_one_layer(ctx, name, layer_name, checks);
    }

    // 8. Composition dry-run.
    let paths = crate::services::config_recipe::ConfigRecipePaths {
        recipes_dir: ctx.config.config_recipe.recipes_dir.clone(),
        configs_dir: ctx.config.config_recipe.configs_dir.clone(),
        profiles_dir: ctx.config.config_recipe.profiles_dir.clone(),
        cache_config: cache_config_path(ctx),
    };
    match crate::services::config_recipe::compose(name, &paths) {
        Ok(composition) => {
            checks.push(ok(
                format!("composition.{name}.dry-run"),
                format!(
                    "merged {} layers, {} top-level keys, {} env vars",
                    composition.layer_paths.len(),
                    composition.merged_config.len(),
                    composition.env.len()
                ),
            ));
            if show_env {
                env_dump.insert(name.to_owned(), redact_secrets(&composition.env));
            }
        }
        Err(err) => {
            checks.push(fail(format!("composition.{name}.dry-run"), err.to_string()));
        }
    }
}

fn check_one_layer(
    ctx: &crate::context::AppContext,
    recipe_name: &str,
    layer_name: &str,
    checks: &mut Vec<CheckResult>,
) {
    let layer_path = ctx
        .config
        .config_recipe
        .configs_dir
        .join(format!("{layer_name}.toml"));
    let check_id = format!("layer.{recipe_name}.{layer_name}");

    match std::fs::metadata(layer_path.as_std_path()) {
        Ok(meta) if meta.is_file() => {}
        Ok(_) => {
            checks.push(fail(
                format!("{check_id}.exists"),
                format!("{layer_path} is not a file"),
            ));
            return;
        }
        Err(err) => {
            checks.push(fail(
                format!("{check_id}.exists"),
                format!("{layer_path}: {err}"),
            ));
            return;
        }
    }
    checks.push(ok(format!("{check_id}.exists"), layer_path.to_string()));

    match crate::services::config_recipe::read_layer(&layer_path) {
        Ok(table) => {
            checks.push(ok(
                format!("{check_id}.parse"),
                format!("{} top-level keys", table.len()),
            ));
            if let Some(env_value) = table.get("env") {
                checks.push(check_layer_env(&check_id, env_value));
            }
        }
        Err(err) => {
            checks.push(fail(format!("{check_id}.parse"), err.to_string()));
        }
    }
}

fn check_active_config_recipe(
    ctx: &crate::context::AppContext,
    active: Option<&str>,
) -> CheckResult {
    let Some(name) = active else {
        return warn(
            "config-recipe.active",
            "stock mode (no active config-recipe)".to_owned(),
        );
    };
    let source = active_config_recipe_source(ctx, name);
    ok("config-recipe.active", format!("{name} (source: {source})"))
}

fn check_ping_config(ctx: &crate::context::AppContext) -> CheckResult {
    match crate::services::account::gate::validate_ping_config_recipe(ctx) {
        Ok(()) => ok(
            "config-recipe.ping-profile",
            "ping profile composes (token probe enabled)",
        ),
        Err(err) => warn(
            "config-recipe.ping-profile",
            format!("ping profile unavailable: {err}"),
        ),
    }
}

fn active_config_recipe_source(ctx: &crate::context::AppContext, name: &str) -> &'static str {
    if ctx.global.config_recipe.as_deref() == Some(name) {
        return "cli";
    }
    if std::env::var("CODEX_SESSION_CONFIG_RECIPE").ok().as_deref() == Some(name) {
        return "env";
    }
    if ctx.config.config_recipe.default.as_deref() == Some(name) {
        return "config.default";
    }
    "fallback"
}

fn check_layer_env(check_id: &str, env_value: &toml::Value) -> CheckResult {
    let toml::Value::Table(table) = env_value else {
        return fail(
            format!("{check_id}.env"),
            "`[env]` must be a table of string values".to_owned(),
        );
    };
    let mut problems = Vec::new();
    for (key, value) in table {
        if !crate::services::config_recipe::is_valid_env_key(key) {
            problems.push(format!("`{key}` must match ^[A-Za-z_][A-Za-z0-9_]*$"));
            continue;
        }
        if key.starts_with("CODEX_SESSION_") {
            problems.push(format!("`{key}` uses reserved CODEX_SESSION_* prefix"));
            continue;
        }
        if !matches!(value, toml::Value::String(_)) {
            problems.push(format!("`{key}` value must be a string scalar"));
        }
    }
    if problems.is_empty() {
        ok(
            format!("{check_id}.env"),
            format!("{} env vars", table.len()),
        )
    } else {
        fail(format!("{check_id}.env"), problems.join("; "))
    }
}

fn check_orphan_layers(ctx: &crate::context::AppContext) -> CheckResult {
    let configs_dir = &ctx.config.config_recipe.configs_dir;
    let recipes_dir = &ctx.config.config_recipe.recipes_dir;
    if !configs_dir.is_dir() {
        return ok(
            "layers.orphan",
            format!(
                "no configs/ directory at {configs_dir} — create it and add \
                at least one layer file (e.g. {configs_dir}/base.toml)"
            ),
        );
    }

    let entries = match std::fs::read_dir(configs_dir.as_std_path()) {
        Ok(entries) => entries,
        Err(err) => {
            return fail(
                "layers.orphan",
                format!("could not read {configs_dir}: {err}"),
            );
        }
    };

    let ReferencedLayers {
        names: referenced,
        unparsed,
        recipes_dir_unreadable,
    } = collect_referenced_layers(recipes_dir);

    if let Some(reason) = recipes_dir_unreadable {
        return fail(
            "layers.orphan",
            format!("could not read config-recipes directory {recipes_dir}: {reason}"),
        );
    }

    let mut orphans = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "toml") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if !referenced.contains(stem) {
            orphans.push(stem.to_owned());
        }
    }

    let mut details: Vec<String> = Vec::new();
    if !orphans.is_empty() {
        orphans.sort();
        details.push(format!("unreferenced: {}", orphans.join(", ")));
    }
    if !unparsed.is_empty() {
        details.push(format!(
            "result indeterminate — {} manifest(s) failed to parse: {}",
            unparsed.len(),
            unparsed.join(", ")
        ));
    }

    if details.is_empty() {
        ok("layers.orphan", "no orphan layers".to_owned())
    } else {
        // If any manifests failed to parse, the orphan set may include
        // false positives, so the overall check is at best advisory.
        warn("layers.orphan", details.join("; "))
    }
}

/// Sweep the user-config tree for legacy shapes that pre-date the codex
/// v0.134+ profile contract. Each finding becomes a single WARN row that
/// cites docs/upstream-codex.md §F6c and names the migration step.
///
/// Scope: every `*.toml` under `configs_dir/` and every `*.config.toml`
/// under `profiles_dir/`, regardless of whether the active
/// manifest references them — `compose()` already validates the
/// referenced subset, this catches the orphans.
fn check_legacy_profile_forms(ctx: &crate::context::AppContext) -> Vec<CheckResult> {
    let mut checks: Vec<CheckResult> = Vec::new();
    let mut seen: BTreeSet<Utf8PathBuf> = BTreeSet::new();

    let cfg_recipe = &ctx.config.config_recipe;
    let configs_dir = cfg_recipe.configs_dir.clone();
    let profiles_dir = cfg_recipe.profiles_dir.clone();
    let legacy_settings_dir = cfg_recipe.config_dir.join("settings");
    let legacy_cache_file = ctx.config.paths.cache_dir.join("settings.toml");

    if legacy_settings_dir.is_dir() {
        checks.push(warn(
            "legacy-settings-dir",
            format!(
                "legacy `{legacy_settings_dir}` directory detected; move \
                its layer files to `{configs_dir}` and extract any \
                `[profiles.<name>]` tables into \
                `{profiles_dir}/<name>.config.toml`. \
                See docs/upstream-codex.md §F6c."
            ),
        ));
    }

    if legacy_cache_file.is_file() {
        let new = ctx.config.paths.cache_dir.join("configs.toml");
        let detail = if new.is_file() {
            format!(
                "legacy cache file `{legacy_cache_file}` detected alongside \
                live cache `{new}`; merge any `[projects]` rows from the \
                legacy file into the live one and delete the legacy file. \
                Do not blindly rename — `{new}` is the live cache. \
                See docs/upstream-codex.md §F6c."
            )
        } else {
            format!(
                "legacy cache file `{legacy_cache_file}` detected; rename \
                to `{new}` BEFORE the next codex-session run (running \
                codex-session creates `{new}` from baseline and the legacy \
                file is no longer consulted). See docs/upstream-codex.md §F6c."
            )
        };
        checks.push(warn("legacy-cache-config", detail));
    }

    let nested_profiles_dir = configs_dir.join("profiles");
    if nested_profiles_dir.is_dir() && nested_profiles_dir != profiles_dir {
        checks.push(warn(
            "config_recipe.legacy_layout",
            format!(
                "obsolete nested `{nested_profiles_dir}` directory detected; \
                move per-profile files to `{profiles_dir}` (sibling of `configs/`). \
                See docs/upstream-codex.md §F6c."
            ),
        ));
    }

    sweep_dir_for_legacy(&configs_dir, &profiles_dir, false, &mut seen, &mut checks);
    sweep_dir_for_legacy(&profiles_dir, &profiles_dir, true, &mut seen, &mut checks);

    checks
}

fn sweep_dir_for_legacy(
    dir: &Utf8Path,
    profiles_dir: &Utf8Path,
    is_profiles_subdir: bool,
    seen: &mut BTreeSet<Utf8PathBuf>,
    checks: &mut Vec<CheckResult>,
) {
    let Ok(entries) = std::fs::read_dir(dir.as_std_path()) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(path) = Utf8PathBuf::try_from(entry.path()) else {
            continue;
        };
        if !path.is_file() {
            continue;
        }
        let Some(file_name) = path.file_name() else {
            continue;
        };
        let matches_suffix = if is_profiles_subdir {
            file_name.ends_with(".config.toml")
        } else {
            // Top-level sweep: any `*.toml` EXCEPT `*.config.toml`, which
            // belongs under `profiles/`. A misplaced `*.config.toml` here
            // is a different problem and should not be confused with a
            // legacy base-layer with `[profiles.*]`.
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("toml"))
                && !file_name.ends_with(".config.toml")
        };
        if !matches_suffix {
            continue;
        }
        if !seen.insert(path.clone()) {
            continue;
        }

        let Ok(text) = std::fs::read_to_string(path.as_std_path()) else {
            continue;
        };
        let Ok(table) = toml::from_str::<toml::Table>(&text) else {
            continue;
        };

        if let Err(err) = crate::services::config_recipe::layer::reject_legacy_profile_syntax(
            &table,
            path.as_ref(),
        ) {
            checks.push(warn(
                "legacy-profile-form",
                format!(
                    "{err}. Move profile keys to \
                    `{profiles_dir}/<name>.config.toml` (bare top-level keys, \
                    no `[profiles.<name>]` header). See docs/upstream-codex.md §F6c.",
                ),
            ));
        }
    }
}

struct ReferencedLayers {
    names: BTreeSet<String>,
    /// Manifest stems whose YAML could not be read or parsed. When this
    /// is non-empty, `names` is incomplete and orphan detection is best
    /// effort.
    unparsed: Vec<String>,
    /// `Some(err)` when the `config-recipes` directory itself could not be read.
    /// In that case `names` is empty and `unparsed` is meaningless;
    /// callers should bubble this up as a hard failure instead of
    /// computing orphans.
    recipes_dir_unreadable: Option<String>,
}

fn collect_referenced_layers(recipes_dir: &Utf8Path) -> ReferencedLayers {
    let mut names = BTreeSet::new();
    let mut unparsed: Vec<String> = Vec::new();
    // Treat "directory does not exist" as the empty reference set (it's
    // a legitimate fresh-install state). Anything else — permission
    // denied, NotADirectory, IO error — is reported back to the caller.
    let entries = match std::fs::read_dir(recipes_dir.as_std_path()) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return ReferencedLayers {
                names,
                unparsed,
                recipes_dir_unreadable: None,
            };
        }
        Err(err) => {
            return ReferencedLayers {
                names,
                unparsed,
                recipes_dir_unreadable: Some(err.to_string()),
            };
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "yaml") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("<unknown>")
            .to_owned();
        let Ok(utf8) = Utf8PathBuf::try_from(path) else {
            unparsed.push(stem);
            continue;
        };
        match crate::services::config_recipe::Manifest::parse(utf8) {
            Ok(m) => {
                for name in m.config_layers {
                    names.insert(name);
                }
            }
            Err(_) => unparsed.push(stem),
        }
    }
    unparsed.sort();
    ReferencedLayers {
        names,
        unparsed,
        recipes_dir_unreadable: None,
    }
}

fn check_xdg_paths() -> CheckResult {
    let xdg = [
        "XDG_CONFIG_HOME",
        "XDG_CACHE_HOME",
        "XDG_STATE_HOME",
        "XDG_RUNTIME_DIR",
    ];
    let mut parts = Vec::new();
    for key in xdg {
        let value = std::env::var(key).unwrap_or_else(|_| "(unset)".to_owned());
        parts.push(format!("{key}={value}"));
    }
    ok("xdg.paths", parts.join(" "))
}

fn check_session_root(ctx: &crate::context::AppContext) -> CheckResult {
    // Use the non-mutating inspector here so `doctor` never creates or
    // chmods the runtime/state session roots. The regular execution path
    // (compose / pass-through) still calls `resolve_session_root`, which
    // does ensure the directories exist with 0o700 permissions.
    match crate::services::session::dir::inspect_session_root(
        ctx.config.paths.runtime_dir.as_deref(),
        &ctx.config.paths.state_dir,
    ) {
        Ok(inspected) => {
            let source = match inspected.root.source {
                crate::services::session::dir::SessionRootSource::Runtime => "runtime",
                crate::services::session::dir::SessionRootSource::State => "state",
            };
            let path = &inspected.root.path;
            let on_runtime_fallback = matches!(
                inspected.root.source,
                crate::services::session::dir::SessionRootSource::Runtime
            );
            let mut notes: Vec<&'static str> = Vec::new();
            if on_runtime_fallback {
                notes.push("runtime fallback (legacy)");
            }
            if inspected.root_missing {
                notes.push("not yet initialized");
            } else if inspected.accounts_subdir_missing {
                notes.push("accounts/ not yet created");
            }

            let detail = if notes.is_empty() {
                format!("{path} (source: {source})")
            } else {
                format!("{path} (source: {source} — {})", notes.join("; "))
            };

            if on_runtime_fallback || inspected.root_missing || inspected.accounts_subdir_missing {
                warn("session.root", detail)
            } else {
                ok("session.root", detail)
            }
        }
        Err(err) => fail("session.root", err.to_string()),
    }
}

fn check_trust_cache(ctx: &crate::context::AppContext) -> CheckResult {
    let cache_path = ctx.config.paths.cache_dir.join("configs.toml");
    let lock_path = ctx.config.paths.cache_dir.join(".configs.toml.lock");

    match std::fs::symlink_metadata(cache_path.as_std_path()) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return ok("session.trust-cache", "no cache yet (fresh install)");
        }
        Err(err) => {
            return fail(
                "session.trust-cache",
                format!("cannot stat {cache_path}: {err}"),
            );
        }
        Ok(meta) if meta.file_type().is_symlink() => {
            return fail("session.trust-cache", format!("{cache_path} is a symlink"));
        }
        Ok(meta) if !meta.is_file() => {
            return fail(
                "session.trust-cache",
                format!("{cache_path} is not a regular file"),
            );
        }
        Ok(_) => {}
    }
    match std::fs::read_to_string(cache_path.as_std_path()) {
        Err(err) => {
            return fail(
                "session.trust-cache",
                format!("cannot read {cache_path}: {err}"),
            );
        }
        Ok(contents) if contents.parse::<toml::Table>().is_err() => {
            return fail(
                "session.trust-cache",
                format!("{cache_path} is not valid TOML"),
            );
        }
        Ok(_) => {}
    }
    if let Ok(lock_meta) = std::fs::metadata(lock_path.as_std_path())
        && let Ok(modified) = lock_meta.modified()
        && let Ok(age) = std::time::SystemTime::now().duration_since(modified)
        && age > std::time::Duration::from_secs(60)
    {
        return warn(
            "session.trust-cache",
            format!("{lock_path} is stale ({}s old)", age.as_secs()),
        );
    }
    ok("session.trust-cache", format!("{cache_path} OK"))
}

fn check_session_permissions(ctx: &crate::context::AppContext) -> CheckResult {
    let Ok(inspected) = crate::services::session::dir::inspect_session_root(
        ctx.config.paths.runtime_dir.as_deref(),
        &ctx.config.paths.state_dir,
    ) else {
        return ok("session.permissions", "session root unresolved — skipped");
    };
    if inspected.root_missing {
        return ok(
            "session.permissions",
            "session root not yet initialized — skipped",
        );
    }
    let mut problems: Vec<String> = Vec::new();
    check_dir_mode(&inspected.root.path, &mut problems);
    let accounts_dir = inspected.root.path.join("accounts");
    if accounts_dir.is_dir() {
        check_dir_mode(&accounts_dir, &mut problems);
    }
    if problems.is_empty() {
        ok(
            "session.permissions",
            "session directories have correct ownership and mode",
        )
    } else {
        warn("session.permissions", problems.join("; "))
    }
}

fn check_dir_mode(path: &camino::Utf8Path, problems: &mut Vec<String>) {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let meta = match std::fs::symlink_metadata(path.as_std_path()) {
        Ok(meta) => meta,
        Err(err) => {
            problems.push(format!("{path}: cannot stat ({err})"));
            return;
        }
    };
    if meta.file_type().is_symlink() {
        problems.push(format!("{path} is a symlink"));
        return;
    }
    if !meta.is_dir() {
        problems.push(format!("{path} is not a directory"));
        return;
    }
    if meta.uid() != current_uid() {
        problems.push(format!(
            "{path} owned by uid {}, expected {}",
            meta.uid(),
            current_uid(),
        ));
    }
    let mode = meta.permissions().mode() & 0o777;
    if mode != 0o700 {
        problems.push(format!("{path} mode is {mode:#o}, expected 0o700"));
    }
}

fn current_uid() -> u32 {
    rustix::process::getuid().as_raw()
}

const fn group_id_source_label(
    source: crate::services::session::group_id::GroupIdSource,
) -> &'static str {
    match source {
        crate::services::session::group_id::GroupIdSource::Flag => "flag",
        crate::services::session::group_id::GroupIdSource::Env => "env",
        crate::services::session::group_id::GroupIdSource::Tty => "tty",
        crate::services::session::group_id::GroupIdSource::Ppid => "ppid",
        crate::services::session::group_id::GroupIdSource::Pid => "pid",
    }
}

fn check_child_binary(ctx: &crate::context::AppContext) -> CheckResult {
    match ctx.resolved_child_with_version_check() {
        Ok((path, version)) => ok(
            "child.binary",
            format!("{path} ({})", version_label(version)),
        ),
        Err(err) => fail("child.binary", err.to_string()),
    }
}

fn check_codex_version_minimum(ctx: &crate::context::AppContext) -> CheckResult {
    use crate::codex_compat::{REQUIRED_CODEX_VERSION, VersionCheck};

    match ctx.resolved_child_with_version_check() {
        Ok((_, VersionCheck::Ok(version))) => ok(
            "codex.version",
            format!("{version} (>= {REQUIRED_CODEX_VERSION})"),
        ),
        Ok((_, VersionCheck::TooOld(version))) => fail(
            "codex.version",
            format!(
                "{version} is below required floor {REQUIRED_CODEX_VERSION}. \
                See docs/upstream-codex.md §F6c."
            ),
        ),
        Ok((_, VersionCheck::Unparseable(raw))) => warn(
            "codex.version",
            format!(
                "could not parse `codex --version` output ({raw:?}); wrapper proceeds but \
                recommended floor is {REQUIRED_CODEX_VERSION}."
            ),
        ),
        Err(err) => fail("codex.version", format!("could not resolve child: {err}")),
    }
}

fn version_label(version: &crate::codex_compat::VersionCheck) -> String {
    match version {
        crate::codex_compat::VersionCheck::Ok(version)
        | crate::codex_compat::VersionCheck::TooOld(version) => version.to_string(),
        crate::codex_compat::VersionCheck::Unparseable(raw) => raw.clone(),
    }
}

fn check_session_inventory(ctx: &crate::context::AppContext) -> CheckResult {
    // Read-only inspector — see `check_session_root` for rationale.
    let Ok(inspected) = crate::services::session::dir::inspect_session_root(
        ctx.config.paths.runtime_dir.as_deref(),
        &ctx.config.paths.state_dir,
    ) else {
        return ok(
            "session.inventory",
            "session root unresolved — skipped".to_owned(),
        );
    };
    let groups_dir = inspected.root.path.join("accounts").to_path_buf();
    let Ok(account_entries) = std::fs::read_dir(groups_dir.as_std_path()) else {
        return ok("session.inventory", "no sessions directory".to_owned());
    };

    let stale_threshold = std::time::Duration::from_secs(60 * 60 * 24 * 7);
    let now = SystemTime::now();
    let mut count = 0_usize;
    let mut stale = 0_usize;
    let mut total_bytes: u64 = 0;
    let mut oldest: Option<SystemTime> = None;

    for account in account_entries.flatten() {
        let Ok(account_name) = account.file_name().into_string() else {
            continue;
        };
        if account_name == ".trash" {
            continue;
        }
        // No-follow checks at every step: a symlinked account dir, groups
        // dir, or session child must not redirect `doctor`'s scan (or the
        // recursive `dir_size`) into arbitrary paths outside the session
        // tree.
        let account_path = account.path();
        let Ok(account_meta) = std::fs::symlink_metadata(&account_path) else {
            continue;
        };
        if account_meta.file_type().is_symlink() || !account_meta.is_dir() {
            continue;
        }
        let groups = account_path.join("groups");
        let Ok(groups_meta) = std::fs::symlink_metadata(&groups) else {
            continue;
        };
        if groups_meta.file_type().is_symlink() || !groups_meta.is_dir() {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&groups) else {
            continue;
        };
        for entry in entries.flatten() {
            let entry_path = entry.path();
            let Ok(meta) = std::fs::symlink_metadata(&entry_path) else {
                continue;
            };
            if meta.file_type().is_symlink() || !meta.is_dir() {
                continue;
            }
            count += 1;
            total_bytes = total_bytes.saturating_add(dir_size(&entry_path));
            if let Ok(mtime) = meta.modified() {
                oldest = match oldest {
                    Some(prev) if prev < mtime => Some(prev),
                    _ => Some(mtime),
                };
                if let Ok(age) = now.duration_since(mtime)
                    && age > stale_threshold
                {
                    stale += 1;
                }
            }
        }
    }

    if count == 0 {
        return ok("session.inventory", "no sessions".to_owned());
    }
    let detail = format!(
        "{count} session(s), {} stale (>7d), total {} KiB",
        stale,
        total_bytes / 1024
    );
    if stale > 0 {
        warn("session.inventory", detail)
    } else {
        ok("session.inventory", detail)
    }
}

fn check_auth_native(ctx: &crate::context::AppContext) -> CheckResult {
    // Render the `inspect_native_health` result. The OS-level predicates
    // live in `services::auth` so the bridge and doctor never disagree on
    // what counts as healthy.
    use crate::services::auth_inspect::{AuthFileHealth, DirHealth, inspect_native_health};

    let health = inspect_native_health(ctx.home_dir());

    let dir_warn: Option<String> = match &health.dir {
        DirHealth::Missing => {
            return ok("auth.native", "no native codex home yet".to_owned());
        }
        DirHealth::Symlink => {
            return fail(
                "auth.native",
                format!("{} is a symlink; bridge will refuse", health.paths.dir),
            );
        }
        DirHealth::NotDirectory => {
            return fail(
                "auth.native",
                format!("{} is not a directory", health.paths.dir),
            );
        }
        DirHealth::WrongOwner { uid, expected } => {
            return fail(
                "auth.native",
                format!(
                    "{} owned by uid {uid}, expected {expected}; bridge will refuse",
                    health.paths.dir,
                ),
            );
        }
        DirHealth::InspectError(err) => {
            return fail(
                "auth.native",
                format!("could not inspect {}: {err}", health.paths.dir),
            );
        }
        DirHealth::OkAt0700 => None,
        // Non-0o700 dir mode is auto-corrected on first login, so it's
        // a warning, not fatal. Continue inspecting `auth.json` below —
        // a bad file beats a fixable directory mode.
        DirHealth::OkNeedsChmod { mode } => Some(format!(
            "{} mode {mode:04o}; will be chmod'd on first login",
            health.paths.dir,
        )),
    };

    let detail = match health.auth {
        AuthFileHealth::DirAbsent => {
            // Unreachable: the dir match above returns for every non-Ok
            // arm. Treat defensively.
            return dir_warn.map_or_else(
                || ok("auth.native", "no login yet".to_owned()),
                |msg| warn("auth.native", msg),
            );
        }
        AuthFileHealth::Missing => {
            return dir_warn.map_or_else(
                || ok("auth.native", "no login yet".to_owned()),
                |msg| warn("auth.native", msg),
            );
        }
        AuthFileHealth::Symlink => {
            return fail("auth.native", "auth.json is a symlink".to_owned());
        }
        AuthFileHealth::Hardlinked => {
            return fail(
                "auth.native",
                "auth.json has multiple hard links".to_owned(),
            );
        }
        AuthFileHealth::WrongOwner { uid, expected } => {
            return fail(
                "auth.native",
                format!("auth.json owned by uid {uid}, expected {expected}"),
            );
        }
        AuthFileHealth::BadMode { mode } => {
            return fail(
                "auth.native",
                format!("auth.json mode {mode:04o} has group/other bits set; bridge will refuse"),
            );
        }
        AuthFileHealth::InspectError(err) => {
            format!("last_refresh unknown ({err})")
        }
        AuthFileHealth::Readable { last_refresh } => last_refresh.map_or_else(
            || "last_refresh unknown".to_owned(),
            |ts| format!("last_refresh {ts}"),
        ),
    };

    match dir_warn {
        Some(msg) => warn("auth.native", format!("{msg}; {detail}")),
        None => ok("auth.native", detail),
    }
}

async fn run_online_checks(
    ctx: &crate::context::AppContext,
    account: &crate::services::account::id::AccountId,
) -> [CheckResult; 2] {
    // Concurrent probe + quota refresh, sharing `account health`'s single-use
    // refresh-token race mitigation (see services::account::online_probe): the
    // seed is refreshed once up-front when expired, and the probe is re-run
    // against the quota-rotated auth file if quota's live refresh rotated the
    // token. This matters because the probe itself DOES persist a rotation
    // (gate::heartbeat_probe → persist_probe_rotation), so without the mitigation
    // doctor --online could re-orphan a single-use refresh token.
    let (probe, quota) =
        crate::services::account::online_probe::probe_and_quota(ctx, account).await;

    let token_check = match probe {
        Ok((Some(true), detail)) => ok("online.token-probe", format!("token valid: {detail}")),
        Ok((Some(false), detail)) => fail(
            "online.token-probe",
            format!("token rejected (401): {detail}"),
        ),
        Ok((None, detail)) => warn(
            "online.token-probe",
            format!("probe inconclusive: {detail}"),
        ),
        Err(err) => warn("online.token-probe", format!("probe failed: {err}")),
    };
    let quota_check = match quota {
        Ok(crate::services::account::quota::QuotaResult::Ok(_)) => {
            ok("online.quota-api", "quota API reachable")
        }
        // API-key auth skips the WHAM request entirely (quota::refresh returns
        // ApiKeyMode without contacting the endpoint), so we cannot claim the
        // quota API was reached — report it as not applicable instead of a
        // false-positive "reachable".
        Ok(crate::services::account::quota::QuotaResult::ApiKeyMode) => warn(
            "online.quota-api",
            "quota unavailable in API-key mode (no quota request made)",
        ),
        Err(err) => warn("online.quota-api", format!("quota fetch failed: {err}")),
    };
    [token_check, quota_check]
}

fn dir_size(path: &std::path::Path) -> u64 {
    let mut total: u64 = 0;
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    for entry in entries.flatten() {
        let entry_path = entry.path();
        // Use symlink_metadata so a symlinked file or directory inside a
        // session dir cannot redirect the recursive walk outside the
        // session tree (e.g. via `auth.json -> /etc/shadow` or
        // `subdir -> /`).
        let Ok(meta) = std::fs::symlink_metadata(&entry_path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_file() {
            total = total.saturating_add(meta.len());
        } else if meta.is_dir() {
            total = total.saturating_add(dir_size(&entry_path));
        }
    }
    total
}

enum DiscoveredRecipes {
    /// `read_dir` succeeded; the vec may be empty if no `.yaml` files
    /// matched.
    Found(Vec<String>),
    /// The `config-recipes` directory does not exist yet (legitimate fresh-install
    /// state).
    NotFound,
    /// `read_dir` failed for a reason other than `NotFound` (permission
    /// denied, `NotADirectory`, etc).
    Unreadable(String),
}

fn discover_recipes(recipes_dir: &Utf8Path) -> DiscoveredRecipes {
    let entries = match std::fs::read_dir(recipes_dir.as_std_path()) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return DiscoveredRecipes::NotFound;
        }
        Err(err) => return DiscoveredRecipes::Unreadable(err.to_string()),
    };
    let mut recipes = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "yaml") {
            continue;
        }
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            recipes.push(stem.to_owned());
        }
    }
    recipes.sort();
    DiscoveredRecipes::Found(recipes)
}

fn cache_config_path(ctx: &crate::context::AppContext) -> Option<Utf8PathBuf> {
    let path = ctx.config.paths.cache_dir.join("configs.toml");
    path.is_file().then_some(path)
}

fn populate_next_steps<'a>(
    checks: impl IntoIterator<Item = &'a CheckResult>,
    next_steps: &mut Vec<String>,
) {
    let checks: Vec<&CheckResult> = checks.into_iter().collect();
    for check in checks
        .iter()
        .copied()
        .filter(|c| c.status == CheckStatus::Fail)
    {
        let hint = hint_for(&check.name);
        next_steps.push(format!("{}: {}", check.name, hint));
    }
    for check in checks
        .iter()
        .copied()
        .filter(|c| c.status == CheckStatus::Warn)
    {
        if is_actionable_warn(&check.name) {
            let hint = hint_for(&check.name);
            next_steps.push(format!("{}: {}", check.name, hint));
        }
    }
}

fn is_actionable_warn(name: &str) -> bool {
    matches!(
        name,
        "account.cooldowns"
            | "config-recipe.ping-profile"
            | "session.trust-cache"
            | "online.quota-api"
    )
}

fn hint_for(name: &str) -> &'static str {
    let suffix = name.rsplit('.').next().unwrap_or("");
    if name.starts_with("manifest.") {
        match suffix {
            "exists" => "create the missing manifest under config-recipes/",
            "parse" => "fix the YAML schema (config-layers: [..])",
            _ => "see check detail",
        }
    } else if name.starts_with("layer.") {
        match suffix {
            "exists" => "create the missing layer file under configs/",
            "parse" => "fix TOML syntax in the layer",
            "env" => "fix the offending key in the layer's [env] table",
            _ => "see check detail",
        }
    } else if name.starts_with("composition.") {
        "see the underlying layer/env failure above"
    } else if name == "session.root" {
        "set XDG_RUNTIME_DIR / XDG_STATE_HOME to a writable, owned directory"
    } else if name == "child.binary" {
        "set CODEX_SESSION_CHILD_BIN or install `codex` on PATH"
    } else if name == "codex.version" {
        "upgrade codex to >= 0.134.0 or set CODEX_SESSION_CHILD_BIN to a compatible binary"
    } else if name == "auth.native" {
        "run `codex-session account add <name>` to create and authenticate an account"
    } else if name == "account.active.auth" {
        "run `codex-session account refresh` to re-authenticate"
    } else if name == "account.cooldowns" {
        "wait for cooldown to expire or run `codex-session account cooldown clear --all`"
    } else if name == "session.account" {
        "run `codex-session account add <name>` to register an account, or pass `--account <name>`"
    } else if name == "config-recipe.ping-profile" {
        "add a ping profile (profiles/ping.config.toml) to enable token probing"
    } else if name == "online.token-probe" {
        "run `codex-session account refresh` to re-authenticate"
    } else if name == "online.quota-api" {
        "check network connectivity or API status"
    } else if name == "session.trust-cache" {
        "delete the stale lock file or fix the corrupt cache"
    } else if name == "session.permissions" {
        "fix directory ownership/permissions: chmod 700"
    } else {
        "see check detail"
    }
}

fn check_account_health(
    accounts: &[DoctorAccountEntry],
    active_name: Option<&str>,
) -> Vec<CheckResult> {
    let mut out = Vec::new();
    if let Some(name) = active_name
        && let Some(active) = accounts.iter().find(|a| a.name == name)
        && !active.has_auth
    {
        out.push(fail(
            "account.active.auth",
            format!("active account '{name}' has no auth.json; codex will fail on first API call"),
        ));
    }
    let cooled: Vec<&str> = accounts
        .iter()
        .filter(|a| a.cooldown_active)
        .map(|a| a.name.as_str())
        .collect();
    if !cooled.is_empty() {
        out.push(warn(
            "account.cooldowns",
            format!(
                "{} account(s) in cooldown: {}",
                cooled.len(),
                cooled.join(", "),
            ),
        ));
    }
    out
}

fn summarize<'a>(checks: impl IntoIterator<Item = &'a CheckResult>) -> CheckSummary {
    let mut summary = CheckSummary::default();
    for c in checks {
        match c.status {
            CheckStatus::Ok => summary.ok += 1,
            CheckStatus::Warn => summary.warn += 1,
            CheckStatus::Fail => summary.fail += 1,
        }
    }
    summary
}

fn redact_secrets(env: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for (key, value) in env {
        let upper = key.to_ascii_uppercase();
        let is_secret = upper.ends_with("_TOKEN")
            || upper.ends_with("_SECRET")
            || upper.ends_with("_KEY")
            || upper.ends_with("_PASSWORD");
        out.insert(
            key.clone(),
            if is_secret {
                "***".to_owned()
            } else {
                value.clone()
            },
        );
    }
    out
}

fn ok(name: impl Into<String>, detail: impl Into<String>) -> CheckResult {
    CheckResult {
        name: name.into(),
        status: CheckStatus::Ok,
        detail: detail.into(),
    }
}

fn warn(name: impl Into<String>, detail: impl Into<String>) -> CheckResult {
    CheckResult {
        name: name.into(),
        status: CheckStatus::Warn,
        detail: detail.into(),
    }
}

fn fail(name: impl Into<String>, detail: impl Into<String>) -> CheckResult {
    CheckResult {
        name: name.into(),
        status: CheckStatus::Fail,
        detail: detail.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_checks_iterates_every_group_in_order() {
        let report = DoctorReport {
            config_recipe: None,
            account: "acct".to_owned(),
            account_source: "auto".to_owned(),
            group_id: "group".to_owned(),
            group_id_source: "test".to_owned(),
            codex_home: Utf8PathBuf::new(),
            accounts: Vec::new(),
            active_account: DoctorActiveAccount {
                name: "acct".to_owned(),
                source: "auto".to_owned(),
            },
            groups: vec![
                CheckGroup {
                    name: "first".to_owned(),
                    checks: vec![ok("a", "ok"), warn("b", "warn")],
                },
                CheckGroup {
                    name: "second".to_owned(),
                    checks: vec![fail("c", "fail")],
                },
            ],
            summary: CheckSummary::default(),
            next_steps: Vec::new(),
            env: BTreeMap::new(),
        };

        let names: Vec<_> = report
            .all_checks()
            .map(|check| check.name.as_str())
            .collect();
        assert_eq!(names, vec!["a", "b", "c"]);
    }
}
