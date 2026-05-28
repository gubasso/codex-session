# Round 03: Heartbeat Probe + Cache File Rename + Doctor Detection

> Plan: configs-rename-split-profiles | Round: 03 of 04 | Complexity: L
> Generated: 2026-05-28 | Repo: /workspaces/codex-session

## Context

Upstream codex CLI v0.134.0 requires per-profile overrides to live in sibling files
`$CODEX_HOME/<name>.config.toml` with bare top-level keys. After round 02, the composer emits
the correct split layout from `configs/profiles/<name>.config.toml`, and the input vocabulary
has been renamed (`settings/` → `configs/`). But the heartbeat probe in
`src/services/account/gate.rs` still synthesizes its own `[profiles.ping]`-shaped TOML and
writes it as a single `config.toml` under the probe's isolated `CODEX_HOME` — which codex now
rejects.

This round:

- Rewrites `extract_ping_config` to read `configs/profiles/ping.config.toml` directly (no
  `[profiles.ping]` reconstruction).
- Rewrites `heartbeat_probe` to write the probe's `$CODEX_HOME/` with an empty base
  `config.toml` plus a sibling `ping.config.toml` (the contents of
  `configs/profiles/ping.config.toml` copied 1:1).
- Renames the cache layer file `$XDG_CACHE_HOME/codex-session/settings.toml` →
  `configs.toml`. Updates `pass_through.rs::cache_settings_target` → `cache_config_target`,
  `ConfigRecipePaths::cache_settings` → `cache_config`, `trust_sync.rs` paths, and every test
  that round-trips trust state.
- Expands `doctor` with explicit legacy-form detection: when an old `settings/` directory
  exists alongside `configs/`, or when any `configs/<layer>.toml` carries `profile = "..."`
  / `[profiles.*]`, doctor surfaces a one-line migration hint pointing at the new layout and
  `docs/upstream-codex.md` §F6c.
- Updates `PingProfileMissing` error text to reference
  `configs/profiles/ping.config.toml`.

Out of this round: any `~/.dotfiles/codex-session/` migration — round 04 owns that.

## Previous Rounds

**Round 01 (expected state):** docs codify the API-compat principle. `README.md` filesystem
layout shows `configs/`, `configs/profiles/<name>.config.toml`, and `configs.toml` (cache).
`docs/upstream-codex.md` §F6b–§F6c describe the v0.134+ contract and legacy rejection.
`CLAUDE.md` § Codex config compatibility states the rule. `~/DocsNNotes/...codex-conventions.md`
documents split-file profiles.

**Round 02 (expected state):** `ConfigRecipeConfig` and `ConfigRecipePaths` use `configs_dir`
(default `config_dir.join("configs")`). `Composition` has `profile_files: Vec<ProfileFileRef>`.
`compose()` reads `configs/profiles/*.config.toml`, validates absence of legacy forms, and
packs raw TOML strings into `Composition.profile_files`. `write_session_artifacts` emits
sibling `<name>.config.toml` files 1:1 next to `config.toml`. Manifest YAML uses
`config-layers:` (required) and `profile-files:` (optional). Doctor messages say "configs/"
instead of "settings/". The cache file on disk is STILL `settings.toml` —
`ConfigRecipePaths.cache_settings` and `pass_through.rs::cache_settings_target` are unchanged.
`tests/account_health_cli.rs` is unchanged (still asserts on `[profiles.ping]`). `gate.rs` is
unchanged beyond the `settings_dir` → `configs_dir` field rename.

## Scope of This Round

**In scope:**

- `src/services/account/gate.rs::extract_ping_config`: read the ping profile file directly
  from `configs/profiles/ping.config.toml` and return its raw bytes. No `[profiles.ping]`
  reconstruction. No reliance on `merged_config.get("profiles")`.
- `src/services/account/gate.rs::heartbeat_probe`: write an empty (or minimal) base
  `config.toml` plus a sibling `ping.config.toml` under the probe's isolated `CODEX_HOME`.
  The `codex --profile ping exec` invocation stays unchanged.
- `src/services/account/gate.rs::validate_ping_config_recipe`: continues delegating to
  `extract_ping_config`. Update error-mapping message to point at the new path.
- `src/services/account/error.rs::PingProfileMissing`: update detail-message construction to
  name `configs/profiles/ping.config.toml`.
- `src/commands/pass_through.rs::cache_settings_target` → `cache_config_target`, returning
  `cache_dir.join("codex-session/configs.toml")`.
- `src/services/config_recipe/composition.rs::ConfigRecipePaths.cache_settings` →
  `cache_config`.
- `src/services/config_recipe/mod.rs::compose()`: rename consumption of the cache field; the
  cache-bootstrap layer label is `"configs"` (already set in round 02). Add a check that
  reads the cache layer raw and rejects legacy profile syntax in it (same helper as round 02).
- `src/services/trust_sync.rs`: any hardcoded reference to `settings.toml` updates to
  `configs.toml`.
- `src/commands/doctor.rs`: add legacy-form detection — scan `configs_dir`, `configs/profiles/`,
  and the cache file for any of:
  - presence of an `old` settings dir at `config_dir.join("settings")`,
  - any base layer `configs/<name>.toml` containing `profile = "..."` or `[profiles.*]`,
  - any profile file `configs/profiles/<name>.config.toml` containing `[profiles.*]`,
  - cache file at the legacy path `cache_dir.join("codex-session/settings.toml")`.
  Each finding surfaces a one-line user-facing error pointing at the doc reference and the
  exact migration step.
- `tests/account_health_cli.rs`: rewrite fixtures to use `configs/profiles/ping.config.toml`.
  Update missing-profile assertion text.
- `tests/trust_sync_roundtrip.rs`, `tests/cmd_config_status.rs`, `tests/cmd_doctor.rs`,
  `tests/support/mod.rs` (rename `cache_settings_path()` → `cache_config_path()`): update
  path strings and any visible CLI message text affected by the cache rename + new doctor
  detection.

**Out of scope:**

- Anything in `~/.dotfiles/codex-session/` (round 04).
- Re-stowing the user's `~/.config/codex-session/` tree on this host (manual user step,
  documented in the round 04 acceptance criteria).

## Current State

### Key Files

- `/workspaces/codex-session/src/services/account/gate.rs`:

  Current `extract_ping_config` (lines 379–418):

  ```rust
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
              settings_dir: ctx.config.config_recipe.settings_dir.clone(),
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
                  composed settings for config-recipe `{recipe_name}`"
              );
              AppError::Account(AccountError::PingProfileMissing { detail })
          })?;

      let mut config = toml::Table::new();
      let mut profiles = toml::Table::new();
      profiles.insert(PING_PROFILE.to_owned(), toml::Value::Table(ping_table.clone()));
      config.insert("profiles".to_owned(), toml::Value::Table(profiles));

      toml::to_string_pretty(&config).map_err(|err| AppError::Other(anyhow::anyhow!("{err}")))
  }
  ```

  After round 02, `composition.profile_files` contains a `ProfileFileRef { name, path,
  raw_toml }` for every emitted profile. Round 03 ditches the legacy `[profiles.ping]`
  reconstruction and consumes `profile_files` instead.

  Current `heartbeat_probe` (lines 439–489 excerpt):

  ```rust
  let ping_config = extract_ping_config(ctx)?;
  // ...
  crate::adapters::fs::atomic_write(&tmp_path.join("auth.json"), &bytes)
      .map_err(crate::services::auth::AuthError::from)?;
  crate::adapters::fs::atomic_write(&tmp_path.join("config.toml"), ping_config.as_bytes())
      .map_err(crate::services::auth::AuthError::from)?;

  // ...
  let mut child = Command::new(binary.as_std_path())
      .args(["--profile", PING_PROFILE, "exec", "--json", "say ok"])
      .env_clear()
      .env("CODEX_HOME", tmp_path.as_str())
  ```

  Round 03 writes the ping content to `tmp_path.join("ping.config.toml")` and writes an empty
  `config.toml`.

- `/workspaces/codex-session/src/services/account/error.rs` — `PingProfileMissing { detail:
  String }` variant. The error rendering in `src/error.rs` includes a suggestion to add
  `[profiles.ping]`. Update the suggestion text.

- `/workspaces/codex-session/src/commands/pass_through.rs` (lines 667–671):

  ```rust
  fn cache_settings_target(ctx: &AppContext) -> Utf8PathBuf {
      ctx.config.paths.cache_dir.join("codex-session/settings.toml")
  }
  ```

  Rename function and update join path.

- `/workspaces/codex-session/src/services/trust_sync.rs` — read it to identify every
  hardcoded `settings.toml` reference (likely string literals in narration / log messages and
  possibly path joins). Update to `configs.toml`.

- `/workspaces/codex-session/src/commands/doctor.rs` — existing checks emit messages at
  lines ≈ 497 ("no configs/ directory" after round 02) and ≈ 1000 ("create the missing layer
  file under configs/"). Add a new "legacy-form sweep" function called from the main doctor
  flow that emits one warning per finding.

- `/workspaces/codex-session/tests/account_health_cli.rs` — current fixture (round 02
  untouched):

  ```rust
  &[("base", "[profiles.other]\nmodel = \"gpt-4.1-nano\"\n")]
  ```

  Plus an assertion:

  ```rust
  .stderr(predicate::str::contains("[profiles.ping]"))
  ```

  Both update to the new path-based fixture and assertion text.

### Existing Patterns

- `gate.rs` uses `crate::services::config_recipe::ConfigRecipePaths` and `compose()` to walk
  the user's recipe. Keep using this exact path — after round 02, `compose()` populates
  `Composition.profile_files`, which is the right consumer-side API.
- Empty base `config.toml` is acceptable input for codex (the wrapper already emits empty
  bases in stock mode — see `write_stock_session_artifacts` in `composition.rs`). For the
  heartbeat probe, an empty `config.toml` plus a sibling `ping.config.toml` is the cleanest
  shape.
- `crate::adapters::fs::atomic_write` is the standard atomic file write used by the heartbeat
  probe — reuse for the new sibling write.
- Doctor uses a `Diagnostics` accumulator pattern (grep for `push_warning` / `push_error`).
  The new legacy-form sweep emits findings the same way.
- Migration-hint text references `docs/upstream-codex.md` §F6c (written in round 01). Keep it
  one line per finding so doctor output stays scannable.

## Implementation Steps

### Step 1: Rewrite `extract_ping_config`

In `src/services/account/gate.rs`, replace `extract_ping_config` with:

```rust
/// Returns the raw TOML bytes of the ping profile file from the active
/// codex-session config-recipe. The bytes are written verbatim to
/// `$CODEX_HOME/ping.config.toml` under the probe's isolated CODEX_HOME.
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
            cache_config: cache_config_path(ctx),
        },
    )?;

    let ping = composition
        .profile_files
        .iter()
        .find(|p| p.name == PING_PROFILE)
        .ok_or_else(|| {
            let detail = format!(
                "profile file `configs/profiles/{PING_PROFILE}.config.toml` \
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
```

Delete the old `cache_settings_path` private helper in this file.

### Step 2: Rewrite `heartbeat_probe` file layout

In `src/services/account/gate.rs::heartbeat_probe`, find the two `atomic_write` calls that
set up the probe's `$CODEX_HOME`. Replace with:

```rust
crate::adapters::fs::atomic_write(&tmp_path.join("auth.json"), &bytes)
    .map_err(crate::services::auth::AuthError::from)?;
// Empty base config.toml — codex requires it to exist; all overrides come from the sibling.
crate::adapters::fs::atomic_write(&tmp_path.join("config.toml"), b"")
    .map_err(crate::services::auth::AuthError::from)?;
crate::adapters::fs::atomic_write(
    &tmp_path.join(format!("{PING_PROFILE}.config.toml")),
    ping_config.as_bytes(),
)
.map_err(crate::services::auth::AuthError::from)?;
```

The `Command::new(binary)...args(["--profile", PING_PROFILE, "exec", "--json", "say ok"])`
invocation stays unchanged.

### Step 3: Update `PingProfileMissing` error rendering

Find the user-facing rendering in `src/error.rs` (grep `PingProfileMissing`). Replace any
existing suggestion text like "add a `[profiles.ping]` section to your codex-session settings"
with:

```text
add `configs/profiles/ping.config.toml` to your codex-session config tree.
The file should contain bare top-level keys (no [profiles.ping] header), e.g.:

    model = "gpt-5.4-mini"
    model_reasoning_effort = "minimal"

See docs/upstream-codex.md §F6b for the codex v0.134+ profile contract.
```

### Step 4: Rename `cache_settings_target` → `cache_config_target`

In `src/commands/pass_through.rs`:

```rust
fn cache_config_target(ctx: &AppContext) -> Utf8PathBuf {
    ctx.config.paths.cache_dir.join("codex-session/configs.toml")
}
```

Update every call site (compiler-driven; should be ≤ 3 places in `pass_through.rs` and
possibly `trust_sync.rs`).

### Step 5: Rename `ConfigRecipePaths.cache_settings` → `cache_config`

In `src/services/config_recipe/composition.rs`:

```rust
pub(crate) struct ConfigRecipePaths {
    pub(crate) recipes_dir: Utf8PathBuf,
    pub(crate) configs_dir: Utf8PathBuf,
    pub(crate) cache_config: Option<Utf8PathBuf>,
}
```

Compiler surfaces every constructor (`mod.rs::compose` caller, `gate.rs::extract_ping_config`,
command modules). Field rename only.

### Step 6: Update `trust_sync.rs`

Read `src/services/trust_sync.rs` end-to-end. Update every hardcoded `settings.toml` to
`configs.toml`. Update every `settings` / `cache_settings` log/narration string to
`configs` / `cache_config`. Verify no other tier-2 references to the old name slip through.

### Step 7: Add doctor legacy-form sweep

In `src/commands/doctor.rs`, add a new function:

```rust
/// Sweep the user-config tree for legacy shapes that pre-date the v0.134+
/// upstream codex contract. Each finding emits one diagnostic with a
/// pointer at docs/upstream-codex.md §F6c and the migration step.
fn check_legacy_profile_forms(
    ctx: &AppContext,
    diagnostics: &mut Diagnostics,
) {
    let cfg_recipe = &ctx.config.config_recipe;
    let configs_dir = &cfg_recipe.configs_dir;
    let legacy_settings_dir = cfg_recipe.config_dir.join("settings");
    let legacy_cache_settings = ctx.config.paths.cache_dir.join("codex-session/settings.toml");

    // Old dir name still on disk.
    if legacy_settings_dir.is_dir() {
        diagnostics.push_warning(format!(
            "legacy `{legacy_settings_dir}` directory detected. \
            Move its layer files to `{configs_dir}` (and extract any \
            `[profiles.<name>]` tables into \
            `{configs_dir}/profiles/<name>.config.toml`). \
            See docs/upstream-codex.md §F6c."
        ));
    }

    // Legacy cache file.
    if legacy_cache_settings.is_file() {
        let new = ctx.config.paths.cache_dir.join("codex-session/configs.toml");
        diagnostics.push_warning(format!(
            "legacy cache file `{legacy_cache_settings}` detected. \
            Rename to `{new}`. Trust state is preserved by content; only \
            the filename moves."
        ));
    }

    // Sweep base layers + profile files for legacy keys.
    sweep_dir_for_legacy(&configs_dir, false, diagnostics);
    sweep_dir_for_legacy(&configs_dir.join("profiles"), true, diagnostics);
}

fn sweep_dir_for_legacy(dir: &Utf8Path, is_profiles_subdir: bool, diagnostics: &mut Diagnostics) {
    let Ok(entries) = std::fs::read_dir(dir.as_std_path()) else { return };
    for entry in entries.flatten() {
        let path = match Utf8PathBuf::try_from(entry.path()) { Ok(p) => p, Err(_) => continue };
        if !path.is_file() { continue; }
        let suffix = if is_profiles_subdir { ".config.toml" } else { ".toml" };
        if !path.as_str().ends_with(suffix) { continue; }
        let Ok(text) = std::fs::read_to_string(path.as_std_path()) else { continue };
        let Ok(table) = toml::from_str::<toml::Table>(&text) else { continue };
        if let Err(err) = crate::services::config_recipe::layer::reject_legacy_profile_syntax(
            &table,
            &path.to_string(),
        ) {
            diagnostics.push_warning(format!(
                "{err}. Move profile keys to \
                `<configs_dir>/profiles/<name>.config.toml` (bare top-level \
                keys, no `[profiles.<name>]` header). See docs/upstream-codex.md §F6c."
            ));
        }
    }
}
```

Wire `check_legacy_profile_forms` into the main doctor entry point (grep for an existing
`check_*` call sequence and add this one). Place it after the existing
no-configs-dir / missing-layer-file checks so the user sees structural problems first and
legacy-form findings second.

### Step 8: Rename test helper `cache_settings_path` → `cache_config_path`

In `tests/support/mod.rs`:

```rust
pub fn cache_config_path(&self) -> Utf8PathBuf {
    self.cache.join("codex-session/configs.toml")
}
```

Update every call site across the test suite (compiler/grep-driven).

### Step 9: Rewrite `tests/account_health_cli.rs` fixtures

Replace the inline `[profiles.ping]` fixtures with the new path-based shape. Example:

```rust
let env = TestEnv::new();
let recipe = env.write_recipe("default", "config-layers:\n  - base\n");
env.write_config_layer("base", "model = \"gpt-5.4\"\nweb_search = \"live\"\n");
env.write_profile_file("ping", "model = \"gpt-5.4-mini\"\nmodel_reasoning_effort = \"minimal\"\n");
// ...
```

Update missing-profile assertion text:

```rust
.stderr(predicate::str::contains("configs/profiles/ping.config.toml"))
```

If the test creates a fixture WITHOUT a ping profile file, assert the new
`PingProfileMissing` detail message contains the path.

### Step 10: Update `tests/trust_sync_roundtrip.rs` and related

- Replace `cache_settings_path()` → `cache_config_path()`.
- Replace any hardcoded `"settings.toml"` literal in expectations → `"configs.toml"`.

### Step 11: Add doctor tests for legacy-form detection

In `tests/cmd_doctor.rs`, add three integration tests:

1. `doctor_warns_on_legacy_settings_dir_present` — create a `<config>/settings/` dir on disk
  alongside `<config>/configs/`, assert doctor stderr contains the migration hint.
2. `doctor_warns_on_legacy_profile_selector_in_layer` —
  `<config>/configs/base.toml` contains `profile = "deep"`, assert doctor surfaces the
  legacy-form warning with `docs/upstream-codex.md §F6c` mentioned.
3. `doctor_warns_on_legacy_profiles_table_in_profile_file` —
  `<config>/configs/profiles/deep.config.toml` contains `[profiles.deep]`, assert doctor
  surfaces the warning.

### Step 12: Verify the build + run gates

```bash
cd /workspaces/codex-session
just lint
just test
```

Also manual-smoke (skip if devcontainer blocks bubblewrap — the round 04 verification covers
host-side run):

```bash
# After re-stowing the test host's ~/.config/codex-session/configs/profiles/ping.config.toml,
# this should succeed without the legacy-form rejection:
codex-session account health
```

### Final Step: Update plan index

Update `.plan/01-todo/configs-rename-split-profiles/README.md`:

1. In the `## Execution Order` table, find the row for round 03.
2. Change `Status` from `todo` to `done`.
3. Change `Completed` from `--` to today's date (`YYYY-MM-DD`).

## Acceptance Criteria

- [ ] `extract_ping_config` returns `composition.profile_files.iter().find(|p| p.name ==
      PING_PROFILE)`'s `raw_toml`. No `merged_config.get("profiles")` reference survives in
      `gate.rs`.
- [ ] `heartbeat_probe` writes the probe's `$CODEX_HOME/` with `auth.json`, an empty
      `config.toml`, and `ping.config.toml` (the raw bytes from
      `configs/profiles/ping.config.toml`).
- [ ] `PingProfileMissing` user-facing error names `configs/profiles/ping.config.toml` and
      cites `docs/upstream-codex.md §F6b`.
- [ ] `cache_config_target` replaces `cache_settings_target` in `pass_through.rs` and returns
      `cache_dir.join("codex-session/configs.toml")`.
- [ ] `ConfigRecipePaths.cache_config` replaces `cache_settings`. All constructors updated.
- [ ] `src/services/trust_sync.rs` has no `settings.toml` references; all paths point at
      `configs.toml`.
- [ ] `doctor` emits a warning on each of: legacy `settings/` dir present, legacy
      `cache/codex-session/settings.toml` present, legacy `profile = "..."` or
      `[profiles.*]` in `configs/<layer>.toml`, legacy `[profiles.*]` in
      `configs/profiles/<name>.config.toml`. Each warning cites
      `docs/upstream-codex.md §F6c`.
- [ ] `tests/account_health_cli.rs` uses `write_profile_file("ping", ...)` and asserts the
      new error text.
- [ ] `tests/cmd_doctor.rs` has the three new legacy-form detection tests, all passing.
- [ ] `tests/support/mod.rs` exposes `cache_config_path()`; no `cache_settings_path()`
      references survive in `tests/`.
- [ ] `just lint` passes.
- [ ] `just test` passes (unit + integration).
- [ ] `just precommit-all` passes (mirrors CI).
- [ ] Plan `README.md` execution order table shows round 03 as `done` with today's date.

## Next Round

Round 04 — Dotfiles propagation — migrates the user's stow source-of-truth at
`~/.dotfiles/codex-session/.config/codex-session/` to the new layout
(`settings/` → `configs/`, `[profiles.*]` extracted into per-file
`configs/profiles/<name>.config.toml`, manifest `default.yaml` updated to use
`config-layers:`). After that round and a re-stow on the host, `codex-session account
health` and `codex-session --account auto exec --profile deep "say ok"` succeed end-to-end on
codex v0.134+.
