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
    pub(crate) profile: Option<String>,
    pub(crate) account: String,
    pub(crate) group_id: String,
    pub(crate) group_id_source: String,
    pub(crate) codex_home: Utf8PathBuf,
    pub(crate) checks: Vec<CheckResult>,
    pub(crate) summary: CheckSummary,
    pub(crate) next_steps: Vec<String>,
    /// Per-profile merged env, keyed by profile name. Empty unless
    /// `--show-env` was passed. In single-profile mode this contains at
    /// most one entry (the active profile); in `--all-profiles` mode it
    /// can contain one entry per profile whose composition succeeded.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) env: BTreeMap<String, BTreeMap<String, String>>,
}

pub(crate) fn run(
    ctx: &crate::context::AppContext,
    args: crate::cli::doctor::DoctorArgs,
) -> Result<u8, crate::error::AppError> {
    let report = build_report(ctx, args);
    let fmt = ctx.global.format.unwrap_or_default();
    ctx.ui.write_doctor(&report, fmt)?;
    Ok(u8::from(report.summary.fail > 0))
}

#[allow(clippy::too_many_lines)]
fn build_report(
    ctx: &crate::context::AppContext,
    args: crate::cli::doctor::DoctorArgs,
) -> DoctorReport {
    let mut checks: Vec<CheckResult> = Vec::new();
    let mut next_steps: Vec<String> = Vec::new();
    let mut env_dump: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let root = crate::services::session::dir::inspect_session_root(
        ctx.config.paths.runtime_dir.as_deref(),
        &ctx.config.paths.state_dir,
    )
    .ok();

    let (account, group_id, group_id_source, codex_home) = match (
        crate::services::session::group_id::current(ctx),
        root.as_ref(),
    ) {
        (Ok(resolved_group), Some(inspected_root)) => {
            let codex_home = crate::services::session::dir::inspect_session_dir(
                &inspected_root.root.path,
                "default",
                resolved_group.id.as_str(),
            )
            .map(|dir| dir.path)
            .unwrap_or_default();
            (
                "default".to_owned(),
                resolved_group.id.as_str().to_owned(),
                group_id_source_label(resolved_group.source).to_owned(),
                codex_home,
            )
        }
        (Ok(resolved_group), None) => (
            "default".to_owned(),
            resolved_group.id.as_str().to_owned(),
            group_id_source_label(resolved_group.source).to_owned(),
            Utf8PathBuf::new(),
        ),
        (Err(err), _) => {
            checks.push(fail("session.group_id", err.to_string()));
            (
                "default".to_owned(),
                "(unresolved)".to_owned(),
                "error".to_owned(),
                Utf8PathBuf::new(),
            )
        }
    };

    // 1. Active profile resolution + source.
    let active = ctx.config.profile.active.clone();
    checks.push(check_active_profile(ctx, active.as_deref()));

    // 2-8. Per-profile checks: manifest, layers, composition, env extraction.
    if args.all_profiles {
        match discover_profiles(&ctx.config.profile.profiles_dir) {
            DiscoveredProfiles::Found(profiles) if profiles.is_empty() => {
                checks.push(warn(
                    "profiles.all",
                    format!(
                        "no profile manifests found in {}",
                        ctx.config.profile.profiles_dir
                    ),
                ));
                next_steps.push(format!(
                    "create a manifest at {}/<name>.yaml",
                    ctx.config.profile.profiles_dir
                ));
            }
            DiscoveredProfiles::Found(profiles) => {
                for name in &profiles {
                    check_one_profile(ctx, name, &mut checks, &mut env_dump, args.show_env);
                }
            }
            DiscoveredProfiles::NotFound => {
                checks.push(warn(
                    "profiles.all",
                    format!(
                        "profiles directory does not exist: {}",
                        ctx.config.profile.profiles_dir
                    ),
                ));
                next_steps.push(format!(
                    "create a manifest at {}/<name>.yaml",
                    ctx.config.profile.profiles_dir
                ));
            }
            DiscoveredProfiles::Unreadable(reason) => {
                checks.push(fail(
                    "profiles.all",
                    format!(
                        "could not read profiles directory {}: {reason}",
                        ctx.config.profile.profiles_dir
                    ),
                ));
            }
        }
    } else if let Some(name) = active.as_deref() {
        check_one_profile(ctx, name, &mut checks, &mut env_dump, args.show_env);
    } else {
        next_steps.push(format!(
            "create a manifest at {}/default.yaml or pass --profile NAME",
            ctx.config.profile.profiles_dir
        ));
    }

    // 9. Orphan settings files.
    checks.push(check_orphan_layers(ctx));

    // 11. XDG paths.
    checks.push(check_xdg_paths());

    // 12. Session root.
    checks.push(check_session_root(ctx));

    // 13. Child binary.
    checks.push(check_child_binary(ctx));

    // 14. Session sidecar inventory.
    checks.push(check_session_inventory(ctx));

    // 15. Native auth bridge health.
    checks.push(check_auth_native(ctx));

    populate_next_steps(&checks, &mut next_steps);

    let summary = summarize(&checks);
    DoctorReport {
        profile: active,
        account,
        group_id,
        group_id_source,
        codex_home,
        checks,
        summary,
        next_steps,
        env: env_dump,
    }
}

fn check_one_profile(
    ctx: &crate::context::AppContext,
    name: &str,
    checks: &mut Vec<CheckResult>,
    env_dump: &mut BTreeMap<String, BTreeMap<String, String>>,
    show_env: bool,
) {
    let manifest_path = ctx.config.profile.profiles_dir.join(format!("{name}.yaml"));

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
    let manifest = match crate::services::profile::Manifest::parse(manifest_path) {
        Ok(m) => {
            checks.push(ok(
                format!("manifest.{name}.parse"),
                format!("{} layers", m.settings_layers.len()),
            ));
            m
        }
        Err(err) => {
            checks.push(fail(format!("manifest.{name}.parse"), err.to_string()));
            return;
        }
    };

    // 5-7. Per-layer existence, parse, env validation.
    for layer_name in &manifest.settings_layers {
        check_one_layer(ctx, name, layer_name, checks);
    }

    // 8. Composition dry-run.
    let paths = crate::services::profile::ProfilePaths {
        profiles_dir: ctx.config.profile.profiles_dir.clone(),
        settings_dir: ctx.config.profile.settings_dir.clone(),
        cache_settings: cache_settings_path(ctx),
    };
    match crate::services::profile::compose(name, &paths) {
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
    profile_name: &str,
    layer_name: &str,
    checks: &mut Vec<CheckResult>,
) {
    let layer_path = ctx
        .config
        .profile
        .settings_dir
        .join(format!("{layer_name}.toml"));
    let check_id = format!("layer.{profile_name}.{layer_name}");

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

    match crate::services::profile::read_layer(&layer_path) {
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

fn check_active_profile(ctx: &crate::context::AppContext, active: Option<&str>) -> CheckResult {
    let Some(name) = active else {
        return warn(
            "profile.active",
            "stock mode (no active profile)".to_owned(),
        );
    };
    let source = active_profile_source(ctx, name);
    ok("profile.active", format!("{name} (source: {source})"))
}

fn active_profile_source(ctx: &crate::context::AppContext, name: &str) -> &'static str {
    if ctx.global.profile.as_deref() == Some(name) {
        return "cli";
    }
    if std::env::var("CODEX_SESSION_PROFILE").ok().as_deref() == Some(name) {
        return "env";
    }
    if ctx.config.profile.default.as_deref() == Some(name) {
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
        if !crate::services::profile::is_valid_env_key(key) {
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
    let settings_dir = &ctx.config.profile.settings_dir;
    let profiles_dir = &ctx.config.profile.profiles_dir;
    if !settings_dir.is_dir() {
        return ok("layers.orphan", "no settings/ directory".to_owned());
    }

    let entries = match std::fs::read_dir(settings_dir.as_std_path()) {
        Ok(entries) => entries,
        Err(err) => {
            return fail(
                "layers.orphan",
                format!("could not read {settings_dir}: {err}"),
            );
        }
    };

    let ReferencedLayers {
        names: referenced,
        unparsed,
        profiles_dir_unreadable,
    } = collect_referenced_layers(profiles_dir);

    if let Some(reason) = profiles_dir_unreadable {
        return fail(
            "layers.orphan",
            format!("could not read profiles directory {profiles_dir}: {reason}"),
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

struct ReferencedLayers {
    names: BTreeSet<String>,
    /// Manifest stems whose YAML could not be read or parsed. When this
    /// is non-empty, `names` is incomplete and orphan detection is best
    /// effort.
    unparsed: Vec<String>,
    /// `Some(err)` when the profiles directory itself could not be read.
    /// In that case `names` is empty and `unparsed` is meaningless;
    /// callers should bubble this up as a hard failure instead of
    /// computing orphans.
    profiles_dir_unreadable: Option<String>,
}

fn collect_referenced_layers(profiles_dir: &Utf8Path) -> ReferencedLayers {
    let mut names = BTreeSet::new();
    let mut unparsed: Vec<String> = Vec::new();
    // Treat "directory does not exist" as the empty reference set (it's
    // a legitimate fresh-install state). Anything else — permission
    // denied, NotADirectory, IO error — is reported back to the caller.
    let entries = match std::fs::read_dir(profiles_dir.as_std_path()) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return ReferencedLayers {
                names,
                unparsed,
                profiles_dir_unreadable: None,
            };
        }
        Err(err) => {
            return ReferencedLayers {
                names,
                unparsed,
                profiles_dir_unreadable: Some(err.to_string()),
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
        match crate::services::profile::Manifest::parse(utf8) {
            Ok(m) => {
                for name in m.settings_layers {
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
        profiles_dir_unreadable: None,
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
    match ctx.resolved_child() {
        Ok(path) => {
            let version = std::process::Command::new(path.as_std_path())
                .arg("--version")
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_owned());
            let detail = version.map_or_else(
                || format!("{path} (version unknown)"),
                |v| format!("{path} ({v})"),
            );
            ok("child.binary", detail)
        }
        Err(err) => fail("child.binary", err.to_string()),
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
    let groups_dir = inspected
        .root
        .path
        .join("accounts")
        .join("default")
        .join("groups");
    let Ok(entries) = std::fs::read_dir(groups_dir.as_std_path()) else {
        return ok("session.inventory", "no sessions directory".to_owned());
    };

    let stale_threshold = std::time::Duration::from_secs(60 * 60 * 24 * 7);
    let now = SystemTime::now();
    let mut count = 0_usize;
    let mut stale = 0_usize;
    let mut total_bytes: u64 = 0;
    let mut oldest: Option<SystemTime> = None;

    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_dir() {
            continue;
        }
        count += 1;
        total_bytes = total_bytes.saturating_add(dir_size(&entry.path()));
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
    use crate::services::auth::{AuthFileHealth, DirHealth, inspect_native_health};

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

fn dir_size(path: &std::path::Path) -> u64 {
    let mut total: u64 = 0;
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_file() {
            total = total.saturating_add(meta.len());
        } else if meta.is_dir() {
            total = total.saturating_add(dir_size(&entry.path()));
        }
    }
    total
}

enum DiscoveredProfiles {
    /// `read_dir` succeeded; the vec may be empty if no `.yaml` files
    /// matched.
    Found(Vec<String>),
    /// The profiles directory does not exist yet (legitimate fresh-install
    /// state).
    NotFound,
    /// `read_dir` failed for a reason other than `NotFound` (permission
    /// denied, `NotADirectory`, etc).
    Unreadable(String),
}

fn discover_profiles(profiles_dir: &Utf8Path) -> DiscoveredProfiles {
    let entries = match std::fs::read_dir(profiles_dir.as_std_path()) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return DiscoveredProfiles::NotFound;
        }
        Err(err) => return DiscoveredProfiles::Unreadable(err.to_string()),
    };
    let mut profiles = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "yaml") {
            continue;
        }
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            profiles.push(stem.to_owned());
        }
    }
    profiles.sort();
    DiscoveredProfiles::Found(profiles)
}

fn cache_settings_path(ctx: &crate::context::AppContext) -> Option<Utf8PathBuf> {
    let path = ctx.config.paths.cache_dir.join("settings.toml");
    path.is_file().then_some(path)
}

fn populate_next_steps(checks: &[CheckResult], next_steps: &mut Vec<String>) {
    for check in checks.iter().filter(|c| c.status == CheckStatus::Fail) {
        let hint = hint_for(&check.name);
        next_steps.push(format!("{}: {}", check.name, hint));
    }
}

fn hint_for(name: &str) -> &'static str {
    let suffix = name.rsplit('.').next().unwrap_or("");
    if name.starts_with("manifest.") {
        match suffix {
            "exists" => "create the missing manifest under profiles/",
            "parse" => "fix the YAML schema (settings-layers: [..])",
            _ => "see check detail",
        }
    } else if name.starts_with("layer.") {
        match suffix {
            "exists" => "create the missing layer file under settings/",
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
    } else {
        "see check detail"
    }
}

fn summarize(checks: &[CheckResult]) -> CheckSummary {
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
