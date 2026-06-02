# Round 01: Check Completeness + Grouped Report Model

> Plan: 04-doctor-refactor-completeness-ux | Round: 01 of 02 | Complexity: L (override) |
> Executor: prex (EF 1.5) | Generated: 2026-05-27 | Updated: 2026-06-02 | Repo: /workspaces/codex-session

## Context

The `codex-session doctor` subcommand validates config-recipes, accounts, sessions, and auth. It
runs ~25 checks and renders them as a flat list. It is missing five diagnostics — `[profiles.ping]`
presence, token validity, quota reachability, trust-sync cache health, and session-directory
permissions — and its flat `Vec<CheckResult>` makes both text and JSON hard to consume.

This round adds the `--online` flag, introduces a `CheckGroup` model, restructures `DoctorReport`
into grouped sections, implements the five new checks (the two network checks run **concurrently**
under `--online`), assigns every existing check to a group, updates the JSON serialization, and
updates tests to compile against the new shape. **Text rendering stays flat in this round** — the
grouped/styled text output is Round 02.

## Previous Rounds

This is the first round — no prior rounds.

## Scope of This Round

**IN scope:**

- Add `--online` flag to `DoctorArgs` (`src/cli/doctor.rs`).
- Create `CheckGroup` struct; restructure `DoctorReport` to `groups: Vec<CheckGroup>`.
- Implement five new check functions in `src/commands/doctor.rs`:
  1. `check_ping_config` — `[profiles.ping]` presence (offline).
  2. `run_online_checks` — concurrent token probe + quota connectivity (online only).
  3. `check_trust_cache` — trust-sync cache file health (offline).
  4. `check_session_permissions` — session directory ownership/mode (offline).
- Assign **all** existing checks to groups (Environment, Accounts, Config Recipe, Session, Auth).
- Update `DoctorReport` JSON serialization to the grouped structure.
- Update `hint_for`/`is_actionable_warn`/`populate_next_steps` for the new check names.
- Update existing tests to compile; add tests for the new checks.

**OUT of scope (Round 02):**

- Grouped/styled text rendering, ✓/⚠/✗ symbols in check rows, human-readable timestamps.
- Online-aware spinner messages.

## Current State

### Key files & exact shapes

`src/cli/doctor.rs` — `DoctorArgs` is `#[derive(Debug, Clone, Copy, Default, clap::Args)]` with two
fields:

```rust
pub(crate) struct DoctorArgs {
    #[arg(long = "all-config-recipes")]
    pub(crate) all_config_recipes: bool,
    #[arg(long)]
    pub(crate) show_env: bool,
}
```

`src/commands/doctor.rs` (~1383 lines):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum CheckStatus { Ok, Warn, Fail }

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct CheckResult { pub(crate) name: String, pub(crate) status: CheckStatus, pub(crate) detail: String }

#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct CheckSummary { pub(crate) ok: usize, pub(crate) warn: usize, pub(crate) fail: usize }

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
    pub(crate) checks: Vec<CheckResult>,          // <-- becomes `groups: Vec<CheckGroup>`
    pub(crate) summary: CheckSummary,
    pub(crate) next_steps: Vec<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) env: BTreeMap<String, BTreeMap<String, String>>,
}
```

`run()` and `build_report()` are **synchronous**:

```rust
pub(crate) fn run(ctx: &crate::context::AppContext, args: crate::cli::doctor::DoctorArgs)
    -> Result<u8, crate::error::AppError> {
    let fmt = ctx.global.format.unwrap_or_default();
    let spinners = SpinnerGroup::new(should_show_spinner(ctx, fmt, false));
    let spinner = spinners.add("Running checks...");
    let report = build_report(ctx, args, Some(&spinner));
    match doctor_finish(&report.summary) { /* finish_ok / finish_err */ }
    ctx.ui.write_doctor(&report, fmt)?;
    Ok(u8::from(report.summary.fail > 0))
}

fn build_report(ctx: &crate::context::AppContext, args: crate::cli::doctor::DoctorArgs,
    progress: Option<&SpinnerHandle>) -> DoctorReport {
    let mut checks: Vec<CheckResult> = Vec::new();
    // ... set_progress(progress, "..."); checks.push(...) sequentially ...
}
```

Constructors and helpers already exist: `ok(name, detail)`, `warn(...)`, `fail(...)`,
`summarize(&[CheckResult])`, `populate_next_steps(&[CheckResult], &mut Vec<String>)`,
`is_actionable_warn(name)` (currently `name == "account.cooldowns"`), `hint_for(name)`,
`redact_secrets(...)`, and `set_progress(progress, msg)`.

Current `hint_for` already maps `codex.version`, `child.binary`, `auth.native`, `account.active.auth`,
`account.cooldowns`, `session.account`, `session.root`, plus the `manifest.*`/`layer.*`/`composition.*`
prefixes.

The child-version check **already exists**:

```rust
fn check_codex_version_minimum(ctx: &crate::context::AppContext) -> CheckResult {
    use crate::codex_compat::{REQUIRED_CODEX_VERSION, VersionCheck};
    match ctx.resolved_child_with_version_check() {
        Ok((_, VersionCheck::Ok(version)))       => ok("codex.version", /* ... */),
        Ok((_, VersionCheck::TooOld(version)))   => fail("codex.version", /* ... */),
        Ok((_, VersionCheck::Unparseable(raw)))  => warn("codex.version", /* ... */),
        Err(err)                                  => fail("codex.version", /* ... */),
    }
}
```

A cache-path helper already exists:

```rust
fn cache_config_path(ctx: &crate::context::AppContext) -> Option<Utf8PathBuf> {
    let path = ctx.config.paths.cache_dir.join("configs.toml");
    path.is_file().then_some(path)
}
```

### Service APIs to reuse

- `src/services/account/gate.rs`:
  - `pub(crate) fn validate_ping_config_recipe(ctx: &AppContext) -> Result<(), AppError>` (**sync**).
  - `pub(crate) async fn probe_token(ctx: &AppContext, account: &AccountId) -> Result<(Option<bool>, String), AppError>` (**async**).
- `src/services/account/quota.rs`:
  - `pub(crate) async fn refresh(ctx: &AppContext, account: &AccountId) -> Result<QuotaResult, QuotaError>` (**async**, forces a network fetch). (`get` is cache-first — do not use it for a connectivity probe.)
- `src/services/session/dir.rs`:
  - `pub(crate) fn inspect_session_root(runtime_dir: Option<&Utf8Path>, state_dir: &Utf8Path) -> Result<InspectedRoot, ConfigError>` returning `InspectedRoot { root: SessionRoot { path, source }, root_missing: bool, accounts_subdir_missing: bool }`.
- `src/services/auth_inspect.rs` — permission-check pattern to copy (uses
  `std::os::unix::fs::{MetadataExt, PermissionsExt}`): `meta.uid()`, `meta.permissions().mode() & 0o777`,
  `meta.file_type().is_symlink()`, expecting `0o700`.
- `src/runtime.rs` — `pub(crate) fn block_on<F: Future>(fut: F) -> F::Output`. Under the active
  multi-thread runtime it uses `block_in_place(|| handle.block_on(fut))`. The doctor dispatch arm
  (`commands::doctor::run(ctx.as_ref(), args)` in `src/cli/dispatch.rs`) is called from an async
  context, so `block_on` is the correct bridge — **do not** make `run`/`build_report` async.
- Prior art for concurrency — `src/commands/account/health.rs:181`:

  ```rust
  let (quota_tuple, probe) = tokio::join!(
      fetch_quota(ctx.as_ref(), account, fast),
      fetch_probe(ctx.as_ref(), account, probe_auth, fast),
  );
  ```

### Current check IDs (every one must be assigned to a group)

`session.account`, `session.group_id`, `account.<id>.cooldown.read`, `config-recipe.active`,
`config-recipes.all`, `manifest.<name>.exists`, `manifest.<name>.parse`, `composition.<name>.dry-run`,
`layer.<recipe>.<layer>.exists`, `layer.<recipe>.<layer>.parse`, `layer.<recipe>.<layer>.env`,
`layers.orphan`, `legacy-settings-dir`, `legacy-cache-config`, `config_recipe.legacy_layout`,
`legacy-profile-form`, `xdg.paths`, `session.root`, `child.binary`, `codex.version`,
`session.inventory`, `auth.native`, `accounts.registry`, `account.active.auth`, `account.cooldowns`.

### Tests

`tests/cmd_doctor.rs` (~27 tests) + `tests/account_doctor.rs`. `doctor_json_shape` currently does:

```rust
let checks = value["checks"].as_array().unwrap();
assert!(!checks.is_empty());
let summary = &value["summary"];
assert_eq!(summary["fail"], 0);
```

## Implementation Steps

### Step 1: Add `--online` flag

In `src/cli/doctor.rs` add (keeps `DoctorArgs: Copy`):

```rust
/// Run network-dependent checks (token probe, quota connectivity), in parallel.
/// Without this flag, doctor performs only fast local checks.
#[arg(long)]
pub(crate) online: bool,
```

### Step 2: Create the `CheckGroup` model

In `src/commands/doctor.rs`:

```rust
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
```

### Step 3: Restructure `DoctorReport`

Replace `pub(crate) checks: Vec<CheckResult>` with `pub(crate) groups: Vec<CheckGroup>`. Keep every
other field. Add an iterator helper and route summarize/next-steps through it:

```rust
impl DoctorReport {
    fn all_checks(&self) -> impl Iterator<Item = &CheckResult> {
        self.groups.iter().flat_map(|g| &g.checks)
    }
}
```

Update `summarize` to accept `impl Iterator<Item = &CheckResult>` (or call `summarize` on a
collected slice from `all_checks()`), and have `populate_next_steps` iterate `report.all_checks()`.

### Step 4: Refactor `build_report` into grouped collection

Keep the **sync signature and `progress: Option<&SpinnerHandle>` parameter.** Replace the single
`checks` vec with one vec per group, then assemble `CheckGroup`s at the end (skip empty groups, and
skip Online entirely unless `args.online` with a resolved account). Group assignment:

- **Environment** (`GROUP_ENVIRONMENT`): `xdg.paths`, `child.binary`, `codex.version`.
- **Accounts** (`GROUP_ACCOUNTS`): `accounts.registry`, `account.<id>.cooldown.read`,
  `account.active.auth`, `account.cooldowns`.
- **Config Recipe** (`GROUP_CONFIG_RECIPE`): `config-recipe.active`, `config-recipes.all`,
  `manifest.*`, `composition.*`, `layer.*`, `layers.orphan`, `legacy-settings-dir`,
  `legacy-cache-config`, `config_recipe.legacy_layout`, `legacy-profile-form`,
  `config-recipe.ping-profile` (new).
- **Session** (`GROUP_SESSION`): `session.account`, `session.group_id`, `session.root`,
  `session.inventory`, `session.trust-cache` (new), `session.permissions` (new).
- **Auth** (`GROUP_AUTH`): `auth.native`.
- **Online** (`GROUP_ONLINE`, only with `--online`): `online.token-probe`, `online.quota-api`.

Display names for Round 02 are derived from these constants; Round 01 only needs correct membership.

### Step 5: `check_ping_config` (offline)

```rust
fn check_ping_config(ctx: &crate::context::AppContext) -> CheckResult {
    match crate::services::account::gate::validate_ping_config_recipe(ctx) {
        Ok(()) => ok("config-recipe.ping-profile", "[profiles.ping] found in composed settings"),
        Err(err) => warn("config-recipe.ping-profile", format!("no [profiles.ping] section: {err}")),
    }
}
```

`warn`, not `fail` — ping is optional, but token validation needs it.

### Step 6: `run_online_checks` — concurrent probe + quota (online only)

Add one async runner that joins the two network calls, then bridge it once from sync `build_report`:

```rust
async fn run_online_checks(
    ctx: &crate::context::AppContext,
    account: &crate::services::account::id::AccountId,
) -> [CheckResult; 2] {
    // Concurrent: overlaps the ~15s probe timeout with the ~10s quota timeout.
    // Prior art: src/commands/account/health.rs:181.
    // Race note: probe + quota refresh on one account can re-orphan a refresh token in the
    // narrow no-group case (health.rs:170-178). Doctor is read-only and never persists a rotated
    // token of its own, so the rare residual is acceptable here.
    let (probe, quota) = tokio::join!(
        crate::services::account::gate::probe_token(ctx, account),
        crate::services::account::quota::refresh(ctx, account),
    );

    let token_check = match probe {
        Ok((Some(true), detail))  => ok("online.token-probe", format!("token valid: {detail}")),
        Ok((Some(false), detail)) => fail("online.token-probe", format!("token rejected (401): {detail}")),
        Ok((None, detail))        => warn("online.token-probe", format!("probe inconclusive: {detail}")),
        Err(err)                  => warn("online.token-probe", format!("probe failed: {err}")),
    };
    let quota_check = match quota {
        Ok(_)    => ok("online.quota-api", "quota API reachable"),
        Err(err) => warn("online.quota-api", format!("quota fetch failed: {err}")),
    };
    [token_check, quota_check]
}
```

In `build_report`, gate on `args.online` and a resolved account, then bridge **once**:

```rust
if args.online {
    if let Some(account) = resolved_account.as_ref() {
        set_progress(progress, "Probing token + quota (parallel)...");
        let [token_check, quota_check] =
            crate::runtime::block_on(run_online_checks(ctx, &account.id));
        online_checks.push(token_check);
        online_checks.push(quota_check);
    } else {
        online_checks.push(warn("online.token-probe", "no resolved account to probe"));
    }
}
```

(`resolved_account` is the `Option<ResolvedAccount { id, source }>` already computed earlier in
`build_report`.)

### Step 7: `check_trust_cache` (offline)

Validate the **current** cache file `<cache_dir>/configs.toml` (lock `.configs.toml.lock`). Reuse
`cache_config_path(ctx)` for existence. This is distinct from the existing `legacy-cache-config`
check, which warns about the _legacy_ `settings.toml`.

```rust
fn check_trust_cache(ctx: &crate::context::AppContext) -> CheckResult {
    let Some(cache_path) = cache_config_path(ctx) else {
        return ok("session.trust-cache", "no cache yet (fresh install)");
    };
    let lock_path = ctx.config.paths.cache_dir.join(".configs.toml.lock");

    match std::fs::symlink_metadata(cache_path.as_std_path()) {
        Ok(meta) if meta.file_type().is_symlink() =>
            return fail("session.trust-cache", format!("{cache_path} is a symlink")),
        Ok(meta) if !meta.is_file() =>
            return fail("session.trust-cache", format!("{cache_path} is not a file")),
        Err(err) =>
            return fail("session.trust-cache", format!("cannot stat {cache_path}: {err}")),
        Ok(_) => {}
    }
    match std::fs::read_to_string(cache_path.as_std_path()) {
        Ok(contents) if contents.parse::<toml::Table>().is_err() =>
            return fail("session.trust-cache", format!("{cache_path} is not valid TOML")),
        Err(err) =>
            return fail("session.trust-cache", format!("cannot read {cache_path}: {err}")),
        Ok(_) => {}
    }
    if let Ok(lock_meta) = std::fs::metadata(lock_path.as_std_path()) {
        if let Ok(age) = lock_meta.modified()
            .and_then(|m| std::time::SystemTime::now().duration_since(m)) {
            if age > std::time::Duration::from_secs(60) {
                return warn("session.trust-cache",
                    format!("{lock_path} is stale ({}s old)", age.as_secs()));
            }
        }
    }
    ok("session.trust-cache", format!("{cache_path} OK"))
}
```

### Step 8: `check_session_permissions` (offline)

Audit session directory ownership/mode beyond the root, following the `auth_inspect.rs` pattern:

```rust
fn check_session_permissions(ctx: &crate::context::AppContext) -> CheckResult {
    let Ok(inspected) = crate::services::session::dir::inspect_session_root(
        ctx.config.paths.runtime_dir.as_deref(),
        &ctx.config.paths.state_dir,
    ) else {
        return ok("session.permissions", "session root unresolved — skipped");
    };
    if inspected.root_missing {
        return ok("session.permissions", "session root not yet initialized — skipped");
    }
    let mut problems: Vec<String> = Vec::new();
    check_dir_mode(&inspected.root.path, &mut problems);
    let accounts_dir = inspected.root.path.join("accounts");
    if accounts_dir.is_dir() {
        check_dir_mode(&accounts_dir, &mut problems);
    }
    if problems.is_empty() {
        ok("session.permissions", "all session directories have correct ownership and mode")
    } else {
        warn("session.permissions", problems.join("; "))
    }
}
```

Add a `check_dir_mode(path, &mut Vec<String>)` helper that uses `MetadataExt::uid` (compare to
`current_uid` as `auth_inspect.rs` does) and `permissions().mode() & 0o777` (flag anything looser
than `0o700`), and flags symlinks.

### Step 9: Group the existing child-version check

No new check. In `build_report`, place `check_codex_version_minimum` (`codex.version`) and
`check_child_binary` (`child.binary`) into the **Environment** group. Keep both IDs unchanged.

### Step 10: Update `hint_for` / `is_actionable_warn`

Add `hint_for` arms:

```text
"config-recipe.ping-profile" => "add [profiles.ping] with a model to your settings layer"
"online.token-probe"         => "run `codex-session account refresh` to re-authenticate"
"online.quota-api"           => "check network connectivity or API status"
"session.trust-cache"        => "delete the stale lock file or fix the corrupt cache"
"session.permissions"        => "fix directory ownership/permissions: chmod 700"
```

Extend `is_actionable_warn` to also return `true` for `config-recipe.ping-profile`,
`session.trust-cache`, and `online.quota-api`.

### Step 11: Update tests

In `tests/cmd_doctor.rs` + `tests/account_doctor.rs`:

- `doctor_json_shape`: navigate the grouped structure. Add a small flatten helper, e.g.:

  ```rust
  let groups = value["groups"].as_array().unwrap();
  let all: Vec<&serde_json::Value> = groups.iter()
      .flat_map(|g| g["checks"].as_array().unwrap().iter())
      .collect();
  assert!(!all.is_empty());
  assert_eq!(value["summary"]["fail"], 0);
  ```

- All other tests: they assert on the **text** output (still flat in this round) and on check
  names/statuses — they keep passing. Only JSON-shape tests change.

Add new tests:

- `doctor_check_ping_config_warns_when_missing` / `_ok_when_present`.
- `doctor_online_flag_runs_network_checks` (Online group present with `--online`).
- `doctor_default_omits_online_group` (no Online group in JSON without `--online`).
- `doctor_trust_cache_ok_when_absent` (fresh install).
- `doctor_trust_cache_warns_on_stale_lock` (create `<cache_dir>/.configs.toml.lock`, age it >60s).

### Final Step: Update plan index

In this directory's `README.md` Execution Order table, set round 01 `Status` → `done` and
`Completed` → today's date (`YYYY-MM-DD`).

## Acceptance Criteria

- [ ] `DoctorArgs` has `--online` and remains `Copy`.
- [ ] `CheckGroup { name, checks }` exists; `DoctorReport` uses `groups: Vec<CheckGroup>`.
- [ ] `DoctorReport::all_checks()` added; `summarize`/`populate_next_steps` route through it.
- [ ] **Every** current check ID is assigned to exactly one group (Environment/Accounts/Config
      Recipe/Session/Auth); Online appears only with `--online` + resolved account.
- [ ] `codex.version` and `child.binary` are in the Environment group (no duplicate version check).
- [ ] `check_ping_config` implemented and tested (`config-recipe.ping-profile`).
- [ ] `run_online_checks` runs probe + quota via a **single** `block_on(tokio::join!)`; cites
      `health.rs:181` and documents the race stance in a comment.
- [ ] `check_trust_cache` validates `<cache_dir>/configs.toml` / `.configs.toml.lock` and is tested.
- [ ] `check_session_permissions` implemented via `inspect_session_root` + `check_dir_mode`.
- [ ] `hint_for`/`is_actionable_warn` cover all new check names.
- [ ] JSON output reflects the grouped structure; `build_report`/`run` stay sync.
- [ ] `just test` and `just lint` pass.
- [ ] README round 01 row is `done` with today's date.

## Next Round

Round 02 rewrites the text branch of `write_doctor` to iterate `report.groups` with section headers
and ✓/⚠/✗ status symbols, renders human-readable timestamps, sets online-aware spinner messages, and
completes the integration-test updates for the new output format.
