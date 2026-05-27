# Round 01: Check Completeness + Grouped Report Model

> Plan: doctor-refactor-completeness-ux | Round: 01 of 02 | Complexity: L (override) | Generated:
> 2026-05-27 | Repo: /workspaces/codex-session

## Context

The `codex-session doctor` subcommand validates the setup of config-recipes, accounts, sessions,
and auth. It currently runs ~15 checks and renders results as a flat list. However, it is missing
several important checks:

- Whether `[profiles.ping]` exists in the composed config-recipe (required for token validation)
- Whether the active account's token actually works against the API
- Whether the quota API is reachable for the active account
- Trust-sync cache settings file health (permissions, existence, stale lock files)
- Session directory permissions beyond just the root
- Child binary (`codex`) version compatibility

Additionally, the `DoctorReport` struct dumps all checks into a flat `Vec<CheckResult>` with no
grouping. This makes both the text output hard to scan and the JSON output hard to consume
programmatically.

This round adds all new checks, introduces `--online` for network-dependent checks, and
restructures `DoctorReport` into grouped sections.

## Previous Rounds

This is the first round — no prior rounds.

## Scope of This Round

**IN scope:**

- Add `--online` flag to `DoctorArgs` in `src/cli/doctor.rs`
- Create `CheckGroup` struct and restructure `DoctorReport` to use `Vec<CheckGroup>`
- Implement 6 new check functions in `src/commands/doctor.rs`:
  1. `check_ping_config` — verifies `[profiles.ping]` exists in composed settings
  2. `check_token_probe` — runs `gate::probe_token()` to verify API connectivity (online only)
  3. `check_quota_connectivity` — calls quota API with short timeout (online only)
  4. `check_trust_cache` — validates cache settings file health
  5. `check_session_permissions` — audits session directory ownership/mode beyond root
  6. `check_child_version_compat` — parses child version and checks compatibility
- Assign all existing checks to their appropriate groups
- Update `DoctorReport` JSON serialization to use grouped structure
- Update existing tests to compile with new struct shape
- Add tests for each new check function

**OUT of scope:**

- Beautiful text rendering (Round 02)
- Design system color integration (Round 02)
- Spinner integration for online checks (Round 02)
- Text output restructuring into sections with headers (Round 02)

## Current State

### Key Files

- `/workspaces/codex-session/src/cli/doctor.rs` — CLI argument parser for doctor. Currently has
  two flags:

  ```rust
  pub(crate) struct DoctorArgs {
      #[arg(long = "all-config-recipes")]
      pub(crate) all_config_recipes: bool,
      #[arg(long)]
      pub(crate) show_env: bool,
  }
  ```

- `/workspaces/codex-session/src/commands/doctor.rs` — Main doctor logic (~1100 lines). Contains:
  - `CheckStatus` enum: `Ok`, `Warn`, `Fail`
  - `CheckResult` struct: `name`, `status`, `detail`
  - `CheckSummary` struct: `ok`, `warn`, `fail` counts
  - `DoctorReport` struct: flat structure with `checks: Vec<CheckResult>`, `summary`,
    `next_steps`, `accounts`, `active_account`, plus top-level metadata
  - `build_report()` function that runs all checks sequentially
  - Individual check functions: `check_active_config_recipe`, `check_one_recipe`,
    `check_one_layer`, `check_layer_env`, `check_orphan_layers`, `check_xdg_paths`,
    `check_session_root`, `check_child_binary`, `check_session_inventory`, `check_auth_native`,
    `check_account_health`
  - Helper functions: `ok()`, `warn()`, `fail()`, `summarize()`, `populate_next_steps()`,
    `hint_for()`, `redact_secrets()`

- `/workspaces/codex-session/src/services/account/gate.rs` — Contains the token probe API:

  ```rust
  pub(crate) fn probe_token(
      ctx: &AppContext,
      account: &AccountId,
  ) -> Result<(Option<bool>, String), AppError>
  ```

  Also `validate_ping_config_recipe(ctx)` which validates that `[profiles.ping]` exists.

- `/workspaces/codex-session/src/services/account/quota.rs` — Quota fetch API:

  ```rust
  pub(crate) fn get(
      // ... parameters for fetching quota
  ) -> Result<Quota, ...>
  ```

- `/workspaces/codex-session/src/services/trust_sync.rs` — Trust cache at
  `<cache_dir>/settings.toml`. The `persist_projects()` function writes to this file using an
  atomic write pattern with a `.settings.toml.lock` lockfile.

- `/workspaces/codex-session/src/services/auth_inspect.rs` — Read-only health inspection:

  ```rust
  pub(crate) fn inspect_native_health(home: &Utf8Path) -> NativeHealth
  ```

- `/workspaces/codex-session/tests/cmd_doctor.rs` — Integration tests (~460 lines). Tests check
  for specific check names and status keywords in stdout. The `doctor_json_shape` test asserts
  on JSON field names like `value["checks"]`, `value["summary"]`.

- `/workspaces/codex-session/tests/account_doctor.rs` — Account-specific doctor tests (~36 lines).

### Existing Patterns

- Check functions return `CheckResult` using `ok()`, `warn()`, `fail()` constructors.
- Check names use dotted notation: `config-recipe.active`, `manifest.<name>.exists`,
  `layer.<recipe>.<layer>.parse`, `session.root`, `child.binary`, `auth.native`.
- `build_report()` collects all checks into a single `Vec<CheckResult>`.
- `populate_next_steps()` generates actionable hints based on failed/warned check names.
- `hint_for()` maps check name prefixes/suffixes to help text.
- Online-capable services live in `src/services/account/` and take `&AppContext`.

## Implementation Steps

### Step 1: Add `--online` flag to DoctorArgs

In `/workspaces/codex-session/src/cli/doctor.rs`, add:

```rust
/// Run network-dependent checks (token probe, quota connectivity).
/// Without this flag, doctor only performs fast local checks.
#[arg(long)]
pub(crate) online: bool,
```

### Step 2: Create CheckGroup model

In `/workspaces/codex-session/src/commands/doctor.rs`, add a new struct:

```rust
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct CheckGroup {
    pub(crate) name: String,
    pub(crate) checks: Vec<CheckResult>,
}
```

Define group name constants:

```rust
const GROUP_ENVIRONMENT: &str = "environment";
const GROUP_ACCOUNTS: &str = "accounts";
const GROUP_CONFIG_RECIPE: &str = "config-recipe";
const GROUP_SESSION: &str = "session";
const GROUP_AUTH: &str = "auth";
const GROUP_ONLINE: &str = "online";
```

### Step 3: Restructure DoctorReport

Modify `DoctorReport` to replace the flat `checks: Vec<CheckResult>` with
`groups: Vec<CheckGroup>`. Keep `summary`, `next_steps`, `accounts`, `active_account`, and the
top-level metadata fields (`config_recipe`, `account`, `account_source`, `group_id`,
`group_id_source`, `codex_home`). Remove the flat `checks` field.

Add a helper method on `DoctorReport` to iterate all checks across groups (for summarization):

```rust
impl DoctorReport {
    fn all_checks(&self) -> impl Iterator<Item = &CheckResult> {
        self.groups.iter().flat_map(|g| &g.checks)
    }
}
```

Update `summarize()` and `populate_next_steps()` to work with the grouped structure by using
`all_checks()`.

### Step 4: Refactor build_report into grouped collection

Restructure `build_report()` to collect checks into groups. Replace the single `let mut checks`
vector with per-group vectors that are assembled into `CheckGroup` instances at the end.

Assignment of existing checks to groups:

- **Environment**: `check_xdg_paths`, `check_child_binary`, `check_child_version_compat` (new)
- **Accounts**: `check_account_health` results, account registry errors
- **Config Recipe**: `check_active_config_recipe`, all `check_one_recipe`/`check_one_layer`
  results, `check_orphan_layers`, `check_ping_config` (new)
- **Session**: `check_session_root`, `check_session_inventory`, `check_session_permissions` (new),
  `check_trust_cache` (new)
- **Auth**: `check_auth_native`, account resolution errors
- **Online** (only when `--online`): `check_token_probe` (new), `check_quota_connectivity` (new)

### Step 5: Implement check_ping_config

Add a new check function that validates the `[profiles.ping]` section exists in the composed
config-recipe settings. This uses the existing
`crate::services::account::gate::validate_ping_config_recipe()`:

```rust
fn check_ping_config(ctx: &crate::context::AppContext) -> CheckResult {
    match crate::services::account::gate::validate_ping_config_recipe(ctx) {
        Ok(()) => ok("config-recipe.ping-profile", "[profiles.ping] found in composed settings"),
        Err(err) => warn(
            "config-recipe.ping-profile",
            format!("no [profiles.ping] section: {err}"),
        ),
    }
}
```

This is a `warn` not a `fail` because ping is optional — the wrapper works without it, but token
validation and some health checks require it.

Add corresponding entry in `hint_for()`:

```text
"config-recipe.ping-profile" => "add [profiles.ping] with a model to your settings layer"
```

### Step 6: Implement check_token_probe (online only)

Add a check that calls `crate::services::account::gate::probe_token()`. This check only runs when
`--online` is passed:

```rust
fn check_token_probe(
    ctx: &crate::context::AppContext,
    account: &crate::services::account::id::AccountId,
) -> CheckResult {
    match crate::services::account::gate::probe_token(ctx, account) {
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
    }
}
```

Add `hint_for()` entry:

```text
"online.token-probe" => "run `codex-session account refresh` to re-authenticate"
```

### Step 7: Implement check_quota_connectivity (online only)

Add a check that attempts to fetch quota for the active account. Uses
`crate::services::account::quota::get()` (or the async equivalent if the spinner plan migrated
it). The check verifies the Wham API is reachable and the account can fetch quota data:

```rust
fn check_quota_connectivity(
    ctx: &crate::context::AppContext,
    account: &crate::services::account::id::AccountId,
) -> CheckResult {
    // Use the existing quota fetch with a reasonable timeout.
    // The exact API shape depends on whether the spinner plan
    // made quota::get async. Adapt to the current state.
    match fetch_quota_for_probe(ctx, account) {
        Ok(_quota) => ok("online.quota-api", "quota API reachable"),
        Err(err) => warn("online.quota-api", format!("quota fetch failed: {err}")),
    }
}
```

This is a `warn` not a `fail` because quota unavailability does not prevent codex from running.

### Step 8: Implement check_trust_cache

Add a check that validates the trust-sync cache settings file at
`<cache_dir>/settings.toml`. Verify:

- File exists (OK) or doesn't (OK — fresh install)
- If it exists: is a regular file (not symlink), readable, valid TOML
- Check for stale `.settings.toml.lock` lockfile (warn if exists and older than 60 seconds)

```rust
fn check_trust_cache(ctx: &crate::context::AppContext) -> CheckResult {
    let cache_path = ctx.config.paths.cache_dir.join("settings.toml");
    let lock_path = ctx.config.paths.cache_dir.join(".settings.toml.lock");

    if !cache_path.exists() {
        return ok("session.trust-cache", "no cache yet (fresh install)");
    }

    // Check for symlink
    match std::fs::symlink_metadata(cache_path.as_std_path()) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return fail("session.trust-cache", format!("{cache_path} is a symlink"));
        }
        Ok(meta) if !meta.is_file() => {
            return fail("session.trust-cache", format!("{cache_path} is not a file"));
        }
        Err(err) => {
            return fail("session.trust-cache", format!("cannot stat {cache_path}: {err}"));
        }
        Ok(_) => {}
    }

    // Validate TOML parseable
    match std::fs::read_to_string(cache_path.as_std_path()) {
        Ok(contents) => {
            if contents.parse::<toml::Table>().is_err() {
                return fail("session.trust-cache", format!("{cache_path} is not valid TOML"));
            }
        }
        Err(err) => {
            return fail("session.trust-cache", format!("cannot read {cache_path}: {err}"));
        }
    }

    // Check for stale lock
    if let Ok(lock_meta) = std::fs::metadata(lock_path.as_std_path()) {
        if let Ok(age) = lock_meta
            .modified()
            .and_then(|mtime| std::time::SystemTime::now().duration_since(mtime))
        {
            if age > std::time::Duration::from_secs(60) {
                return warn(
                    "session.trust-cache",
                    format!("{lock_path} is stale ({}s old)", age.as_secs()),
                );
            }
        }
    }

    ok("session.trust-cache", format!("{cache_path} OK"))
}
```

### Step 9: Implement check_session_permissions

Add a check that audits session directory permissions beyond just the root. Walk the session tree
(root → accounts → groups) and verify each directory is owned by the current user and has mode
0o700 or stricter. Follow the same no-symlink policy as `check_session_inventory`:

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
    // Check root permissions
    check_dir_mode(&inspected.root.path, &mut problems);
    // Check accounts/ subdir if it exists
    let accounts_dir = inspected.root.path.join("accounts");
    if accounts_dir.is_dir() {
        check_dir_mode(&accounts_dir, &mut problems);
    }

    if problems.is_empty() {
        ok("session.permissions", "all session directories have correct permissions")
    } else {
        warn("session.permissions", problems.join("; "))
    }
}
```

Use `rustix::fs::statx` or `std::os::unix::fs::MetadataExt` (already used elsewhere) for
permission checks. The exact implementation should follow patterns already used in `auth_inspect.rs`
for permission checking.

### Step 10: Implement check_child_version_compat

Add a check that parses the child binary's version output and checks for known compatibility
requirements:

```rust
fn check_child_version_compat(ctx: &crate::context::AppContext) -> CheckResult {
    let Ok(path) = ctx.resolved_child() else {
        return ok("environment.child-version", "child binary unresolved — skipped");
    };

    let version_output = std::process::Command::new(path.as_std_path())
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned());

    let Some(version_str) = version_output else {
        return warn("environment.child-version", "could not determine child version");
    };

    // Parse version and check known compatibility constraints.
    // The exact version parsing depends on the codex version format.
    ok("environment.child-version", format!("{version_str} — compatible"))
}
```

Note: The existing `check_child_binary` already captures version info. Consider whether to merge
these or keep them separate. The version compat check adds the compatibility assertion; the binary
check just verifies existence. Keep them separate — `check_child_binary` (renamed to
`environment.child-binary`) confirms the binary exists; `check_child_version_compat`
(`environment.child-version`) confirms the version is compatible.

### Step 11: Update hint_for and populate_next_steps

Add entries to `hint_for()` for all new check names:

```rust
"config-recipe.ping-profile" => "add [profiles.ping] with a model to your settings layer"
"online.token-probe" => "run `codex-session account refresh` to re-authenticate"
"online.quota-api" => "check network connectivity or API status"
"session.trust-cache" => "delete the stale lock file or corrupt cache"
"session.permissions" => "fix directory permissions: chmod 700"
"environment.child-version" => "update codex to a compatible version"
```

Update `is_actionable_warn()` to include the new check names that should generate next-steps
entries when they warn (at minimum `config-recipe.ping-profile` and `session.trust-cache`).

### Step 12: Update existing tests

Update `/workspaces/codex-session/tests/cmd_doctor.rs` and
`/workspaces/codex-session/tests/account_doctor.rs` to account for the new `DoctorReport`
structure:

- `doctor_json_shape`: Update to navigate grouped structure. Instead of
  `value["checks"].as_array()`, use `value["groups"]` and find checks within groups.
- `doctor_happy_path_exits_zero`: Update string assertions for any changed output format.
  The text rendering hasn't changed yet (Round 02), but check names in groups may cause different
  output ordering.
- All other tests: Ensure they still find their expected check names in output.

Add new tests:

- `doctor_check_ping_config_warns_when_missing`: Verify that without `[profiles.ping]`, the check
  warns.
- `doctor_check_ping_config_ok_when_present`: Verify that with `[profiles.ping]`, the check passes.
- `doctor_online_flag_runs_network_checks`: Verify that `--online` causes online group checks to
  appear.
- `doctor_default_omits_online_group`: Verify that without `--online`, the online group is absent
  from JSON output.
- `doctor_trust_cache_ok_when_absent`: Fresh install has no cache — should be OK.
- `doctor_trust_cache_warns_on_stale_lock`: Create a stale `.settings.toml.lock` and verify warn.

### Final Step: Update plan index

Update the plan's `README.md` (in the same directory as this round file) to record completion:

1. In the `## Execution Order` table, find the row for round 01.
2. Change `Status` from `todo` to `done`.
3. Change `Completed` from `--` to today's date (`YYYY-MM-DD`).

## Acceptance Criteria

- [ ] `DoctorArgs` has `--online` flag
- [ ] `CheckGroup` struct exists with `name` and `checks` fields
- [ ] `DoctorReport` uses `groups: Vec<CheckGroup>` instead of flat `checks: Vec<CheckResult>`
- [ ] All existing checks are assigned to their correct group
- [ ] `check_ping_config` implemented and tested
- [ ] `check_token_probe` implemented (runs only with `--online`)
- [ ] `check_quota_connectivity` implemented (runs only with `--online`)
- [ ] `check_trust_cache` implemented and tested
- [ ] `check_session_permissions` implemented
- [ ] `check_child_version_compat` implemented
- [ ] `hint_for()` covers all new check names
- [ ] JSON output reflects grouped structure
- [ ] All existing tests pass with updated assertions
- [ ] New tests cover each new check
- [ ] `just test` passes
- [ ] `just lint` passes
- [ ] Plan `README.md` execution order table shows round 01 as `done` with today's date

## Next Round

Round 02 rewrites the text rendering in `src/ui/mod.rs` to use the design system styles, grouped
section headers with visual separation, status symbols (✓/⚠/✗), colored summary banner, styled
account table within the doctor output, spinner integration for `--online` checks, and full test
suite updates for the new output format.
