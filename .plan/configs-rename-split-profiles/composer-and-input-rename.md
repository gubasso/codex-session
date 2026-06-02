# Round 02: Composer + Input Rename (`settings/` → `configs/`)

> Plan: configs-rename-split-profiles | Round: 02 of 04 | Complexity: L
> Generated: 2026-05-28 | Repo: /workspaces/codex-session

## Context

Upstream codex CLI v0.134.0 hardened its profile config contract: `$CODEX_HOME/config.toml` must
not contain `profile = "..."` or `[profiles.*]`, and per-profile overrides live in sibling files
`$CODEX_HOME/<name>.config.toml` with bare top-level keys. codex-session's composer currently
deep-merges all `settings/*.toml` input layers into a single `config.toml` that still carries the
legacy `[profiles.*]` shape — codex rejects this.

This round retools the composer to match upstream's input contract and renames the input
vocabulary from `settings/` to `configs/`:

- Add a `configs/profiles/<name>.config.toml` input layout. Each profile file is emitted 1:1 to
  `$CODEX_HOME/<name>.config.toml`. No deep-merge across profile layers (codex-native model).
- Reject legacy `profile = "..."` selector and `[profiles.*]` tables at the input layer
  (`configs/<layer>.toml`) and refuse to emit them in `$CODEX_HOME/config.toml`. No compat shim.
- Rename the manifest YAML field `settings-layers:` → `config-layers:` and add optional
  `profile-files:` list. When `profile-files:` is omitted, every file under `configs/profiles/`
  is emitted.
- Rename `ConfigRecipeConfig::settings_dir` → `configs_dir` everywhere in the crate.

Out of this round: the heartbeat probe (`gate.rs`), the cache-layer file rename
(`settings.toml` → `configs.toml`), and doctor's user-facing migration messages — all live in
round 03. Rationale: this round produces a stable composer foundation that round 03 then plugs
the consumer side into. Splitting them prevents one prex run from juggling both the input rename
and the cache-file rename at once.

Principle citation (round 01): `CLAUDE.md` § Codex config compatibility and
`docs/upstream-codex.md` §F6b–§F6c are the source of truth for emission shape and rejected
forms.

## Previous Rounds

**Round 01 (expected state):** `CLAUDE.md` has a new § Codex config compatibility section.
`README.md` filesystem layout shows `configs/`, `configs/profiles/<name>.config.toml`, manifest
field `config-layers:`, and cache file `configs.toml`. `docs/upstream-codex.md` §F6b is rewritten
to the v0.134+ contract and §F6c documents the legacy-form rejection. The principle is
written down but no Rust file has been touched yet — both the on-disk dir name and the cache
file name are still legacy on disk.

## Scope of This Round

**In scope:**

- Rename `ConfigRecipeConfig::settings_dir` → `configs_dir` everywhere in `src/` (compiler-driven
  rename; ~10 call sites).
- Manifest schema: YAML key `settings-layers:` → `config-layers:`. Add optional `profile-files:`
  array. Update `Manifest` struct, deserializer, validator, and error messages.
- `ConfigRecipePaths`: rename `settings_dir` → `configs_dir`. Add derived
  `profiles_dir = configs_dir.join("profiles")` (no separate config key).
- `Composition`: add `profile_files: Vec<ProfileFileRef>`. Each ref holds `name`, source path,
  and raw TOML string.
- `compose()`: collect profile files (manifest-declared or directory scan), read each as a raw
  string AND parse to validate legacy-form rejection, return packed into `Composition`.
- `layer::reject_legacy_profile_syntax(table) -> Result<(), ConfigError>`: new helper. Called for
  every input layer (base + profile files).
- `write_session_artifacts`: after writing `config.toml`, iterate `profile_files` and write each
  to `session_dir.join(format!("{name}.config.toml"))`. Validate `merged_config` carries no
  `profile` key and no `profiles` table — bug-net for the base path.
- `ComposeSidecar`: add `profiles: Vec<ProfileFileRef>` field.
- `tests/support/mod.rs`: rename helpers (`write_settings_layer` → `write_config_layer`,
  `settings_dir()` → `configs_dir()`, etc.) and add `write_profile_file(name, body)`.
- Composer test files: `tests/config_recipe_composition.rs`, `tests/config_recipe_errors.rs`,
  `tests/cmd_config_recipe_compose.rs`, `tests/cmd_config_recipe_show.rs`,
  `tests/cmd_config_recipe_list.rs`. Update path strings and YAML field names; add new tests for
  split-emit and legacy-form rejection at input layer + profile file layer.

**Out of scope (deferred):**

- `src/services/account/gate.rs` and `tests/account_health_cli.rs` (round 03).
- Cache file `settings.toml` → `configs.toml` rename in `pass_through.rs`, `trust_sync.rs`,
  doctor messages (round 03).
- `~/.dotfiles/codex-session/` migration (round 04).

## Current State

### Key Files

- `/workspaces/codex-session/src/services/config_recipe/composition.rs` — `ConfigRecipePaths`,
  `Composition`, `LayerRef`, `LayerSource`, `ComposeSidecar`, `write_session_artifacts`,
  `write_stock_session_artifacts`, `write_atomic`.

  Current `ConfigRecipePaths` (lines 14–19):

  ```rust
  pub(crate) struct ConfigRecipePaths {
      pub(crate) recipes_dir: Utf8PathBuf,
      pub(crate) settings_dir: Utf8PathBuf,
      pub(crate) cache_settings: Option<Utf8PathBuf>,
  }
  ```

  Current `Composition` (lines 21–32):

  ```rust
  pub(crate) struct Composition {
      pub(crate) manifest_path: Utf8PathBuf,
      pub(crate) layer_paths: Vec<LayerRef>,
      pub(crate) merged_config: toml::Table,
      pub(crate) env: BTreeMap<String, String>,
      pub(crate) baseline_projects: Option<toml::Table>,
  }
  ```

  Current `write_session_artifacts` (lines 63–93) writes `config.toml` only and a JSON sidecar.
  No per-profile sibling files. No legacy-form validation.

- `/workspaces/codex-session/src/services/config_recipe/mod.rs` — `compose()` entry (≈ lines
  22–80). Reads cache layer + recipe layers, deep-merges, extracts `[env]`, snapshots
  baseline `[projects]`.

  Hardcoded layer name `"settings"` for cache bootstrap (≈ line 44):

  ```rust
  layer_paths.push(LayerRef {
      name: "settings".to_owned(),
      path: cache.clone(),
      source: LayerSource::CacheBootstrap,
  });
  ```

  This is the cache layer label, not the dir name — but the label should still rename to
  `"configs"` for consistency in observability sidecar output.

- `/workspaces/codex-session/src/services/config_recipe/layer.rs` — `read_layer` (lines 13–28),
  `deep_merge` (lines 30–45), `extract_env` (lines 47–85). The new
  `reject_legacy_profile_syntax(table) -> Result<(), ConfigError>` helper goes here.

- `/workspaces/codex-session/src/services/config_recipe/manifest.rs` — `Manifest` struct (lines
  11–79) with `settings_layers: Vec<String>` (YAML field `"settings-layers"`).

  Excerpt of current parser (≈ lines 18–25):

  ```rust
  pub(crate) struct Manifest {
      #[serde(rename = "settings-layers")]
      pub(crate) settings_layers: serde_yaml_ng::Value,
  }
  ```

  Validator (lines 34–64) enforces non-empty array of `[a-z0-9._-]+` strings.

- `/workspaces/codex-session/src/config/mod.rs` — `ConfigRecipeConfig` (lines 57–66):

  ```rust
  pub struct ConfigRecipeConfig {
      pub default: Option<String>,
      pub config_dir: Utf8PathBuf,
      pub recipes_dir: Utf8PathBuf,
      pub settings_dir: Utf8PathBuf,
      pub active: Option<String>,
  }
  ```

  `FileConfigRecipeConfig` (≈ lines 163–170) is the TOML deserialize layer with
  `settings_dir: Option<Utf8PathBuf>`. Default constructor at lines 276–282:

  ```rust
  settings_dir: config_dir.join("settings"),
  ```

- `/workspaces/codex-session/src/commands/doctor.rs` — strings at lines ≈ 497 ("no settings/
  directory") and ≈ 1000 ("create the missing layer file under settings/"). Both rename to
  `configs/` in this round.
- `/workspaces/codex-session/src/commands/pass_through.rs` — `cache_settings_target()` near
  lines 667–671 returning `cache_dir.join("codex-session/settings.toml")`. **Stays as
  `settings.toml` in this round** — round 03 owns the cache file rename.
- All other consumers of `settings_dir`: `src/services/account/{cooldown,resolver,registry,
  selector}.rs`, `src/services/session/group_id.rs`. Compiler-driven rename — the field name
  change forces every call site to update.

- `/workspaces/codex-session/tests/support/mod.rs`:

  Current helpers (≈ lines 260–299):

  ```rust
  pub fn write_settings_layer(&self, name: &str, body: &str) -> Utf8PathBuf {
      let path = self.settings_dir().join(format!("{name}.toml"));
      // ...
  }
  pub fn settings_dir(&self) -> Utf8PathBuf {
      self.config_home.join("codex-session/settings")
  }
  pub fn cache_settings_path(&self) -> Utf8PathBuf {
      self.cache.join("codex-session/settings.toml")
  }
  ```

- `/workspaces/codex-session/tests/config_recipe_composition.rs` — existing tests:
  `config_recipe_compose_recurses_tables_and_replaces_arrays_scalars`,
  `config_recipe_compose_extracts_env_from_output_config`,
  `config_recipe_compose_prepends_cache_layer`,
  `config_recipe_compose_preserves_machine_local_projects_table`.
- `/workspaces/codex-session/tests/config_recipe_errors.rs` — manifest + layer error cases.

### Existing Patterns

- Compiler-driven rename: change `settings_dir` → `configs_dir` in the struct; `cargo build`
  surfaces every consumer. Same for the manifest field `settings_layers` → `config_layers`.
- Error variants live in `crate::config::ConfigError`. The new
  `LegacyProfileSyntax { location: String, reason: String }` variant follows the existing pattern
  (named struct variant). Reason text cites `docs/upstream-codex.md` §F6c and the upstream
  Advanced Configuration URL.
- Tests use `tempfile`-backed `Env` from `tests/support/mod.rs`. The new
  `write_profile_file(name, body)` writes under `self.configs_dir().join("profiles")`.
- `toml::Table::get("profile")` and `toml::Table::get("profiles")` are the standard sentinel
  checks (TOML keys, case-sensitive).
- `write_atomic` (composition.rs lines 144–163) is reused for every emitted file — same
  tempfile-then-rename ceremony, same `ConfigError::Io` mapping.

## Implementation Steps

### Step 1: Add `reject_legacy_profile_syntax` helper in layer.rs

In `src/services/config_recipe/layer.rs`, add a new public-in-crate function after
`extract_env`:

```rust
/// Reject the legacy profile shapes that codex v0.134+ no longer accepts.
///
/// This check runs on every input layer (base config layers and profile files)
/// and on the emitted base `config.toml` as a final bug net. The wrapper's
/// contract is that its output mirrors codex's input — see
/// `docs/upstream-codex.md` §F6b–§F6c and `CLAUDE.md` § Codex config
/// compatibility.
pub(crate) fn reject_legacy_profile_syntax(
    table: &toml::Table,
    where_: &str,
) -> Result<(), crate::config::ConfigError> {
    if table.contains_key("profile") {
        return Err(crate::config::ConfigError::LegacyProfileSyntax {
            location: where_.to_owned(),
            reason: "top-level `profile = \"...\"` selector is no longer \
              accepted; move the override into a \
              `configs/profiles/<name>.config.toml` file and select with \
              `--profile <name>` on the CLI"
                .to_owned(),
        });
    }
    if table.contains_key("profiles") {
        return Err(crate::config::ConfigError::LegacyProfileSyntax {
            location: where_.to_owned(),
            reason: "`[profiles.<name>]` tables are no longer accepted; move \
              each profile's keys into a separate \
              `configs/profiles/<name>.config.toml` file with bare \
              top-level keys (no `[profiles.<name>]` header)"
                .to_owned(),
        });
    }
    Ok(())
}
```

Add unit tests in the same file for: legacy `profile` key → error, legacy `profiles` table →
error, clean table → ok, table with both → error on `profile` (first sentinel).

### Step 2: Add `ConfigError::LegacyProfileSyntax` variant

In `src/config/mod.rs` (where `ConfigError` is defined — grep `pub enum ConfigError`), add the
variant:

```rust
LegacyProfileSyntax {
    location: String,
    reason: String,
},
```

With `Display` impl text:

```text
legacy profile syntax in {location}: {reason}. See docs/upstream-codex.md §F6c.
```

Make sure the variant maps to exit code `EX_CONFIG` (78) per the existing pattern in
`src/error.rs`.

### Step 3: Rename `settings_dir` → `configs_dir` in `ConfigRecipeConfig`

In `src/config/mod.rs`:

- `ConfigRecipeConfig` field: `settings_dir` → `configs_dir`.
- `FileConfigRecipeConfig` field: `settings_dir` → `configs_dir`. Update the `#[serde(default)]`
  / `#[serde(rename = "settings-dir")]` attribute if present to `"configs-dir"` (or whatever
  the codebase uses — verify by reading the surrounding code).
- Default constructor: `config_dir.join("settings")` → `config_dir.join("configs")`.

Compiler will surface every consumer. Update each call site by renaming the field. Consumers
identified during exploration: `src/services/account/{cooldown,resolver,registry,selector}.rs`,
`src/services/session/group_id.rs`, `src/services/config_recipe/mod.rs`,
`src/services/config_recipe/composition.rs`, `src/services/account/gate.rs` (touched lightly
this round — only the field reference; full `gate.rs` overhaul is round 03),
`src/commands/pass_through.rs`, `src/commands/doctor.rs`.

### Step 4: Rename in `ConfigRecipePaths` and add `profiles_dir`

In `src/services/config_recipe/composition.rs`, change the struct to:

```rust
pub(crate) struct ConfigRecipePaths {
    pub(crate) recipes_dir: Utf8PathBuf,
    pub(crate) configs_dir: Utf8PathBuf,
    pub(crate) cache_settings: Option<Utf8PathBuf>,
}

impl ConfigRecipePaths {
    pub(crate) fn profiles_dir(&self) -> Utf8PathBuf {
        self.configs_dir.join("profiles")
    }
}
```

Keep `cache_settings` named `cache_settings` for now — round 03 renames it to `cache_config`
along with the on-disk file rename.

Update every constructor site (grep for `ConfigRecipePaths {`): they all live near
`compose()` callers (`mod.rs::compose`, `gate.rs::extract_ping_config`, command modules). Field
rename only.

### Step 5: Add profile-file collection to `compose()`

In `src/services/config_recipe/mod.rs::compose()`:

1. After parsing the manifest and the base config layers, build a `Vec<ProfileFileRef>` to pack
   into the returned `Composition`.

2. Determine which profile files to collect:

- If the manifest's optional `profile-files:` array is present, take exactly those names. For
  each `name`, read `paths.profiles_dir().join(format!("{name}.config.toml"))`. Missing file
  is an error with a clear message ("manifest declares profile-file `{name}` but
  `{path}` does not exist").
- If the manifest's `profile-files:` is absent, scan `paths.profiles_dir()` for
  `*.config.toml` entries (sorted alphabetically for determinism) and emit all of them. Empty
  or missing dir → empty Vec.

1. For each collected file, do BOTH:

- Read the raw bytes (for verbatim 1:1 emission).
- Parse to `toml::Table` and call `layer::reject_legacy_profile_syntax(&table, &format!("profile file`{name}.config.toml`"))`.

1. Pack into `Composition.profile_files: Vec<ProfileFileRef>` where:

```rust
pub(crate) struct ProfileFileRef {
    pub(crate) name: String,
    pub(crate) path: Utf8PathBuf,
    pub(crate) raw_toml: String,
}
```

(Add to `composition.rs` next to `LayerRef`. Derive `Debug, Clone, Serialize` with
`#[serde(rename_all = "kebab-case")]`.)

1. Also call `reject_legacy_profile_syntax(&merged_config, "merged base config")` AFTER
   `[env]` extraction and `[projects]` snapshot, BEFORE returning the `Composition`. This
   catches the case where a `configs/<layer>.toml` carries `[profiles.deep]` — the same check
   runs at layer-read time in step 6, but doing it again on the merged result defends against
   future merge logic changes.

2. **Layer-read-time check:** in the loop that reads each `configs/<layer>.toml`, immediately
   after `read_layer()` returns the `toml::Table`, call
   `reject_legacy_profile_syntax(&table, &format!("config layer`{name}.toml`"))`. Same for the
   cache layer.

### Step 6: Modify `write_session_artifacts` to emit profile siblings

In `src/services/config_recipe/composition.rs`:

```rust
pub(crate) fn write_session_artifacts(
    composition: &Composition,
    session_dir: &Utf8Path,
) -> Result<(), crate::config::ConfigError> {
    std::fs::create_dir_all(session_dir.as_std_path())?;

    // Final bug-net: refuse to emit legacy shape into $CODEX_HOME/config.toml.
    crate::services::config_recipe::layer::reject_legacy_profile_syntax(
        &composition.merged_config,
        "emitted config.toml",
    )?;

    let config_path = session_dir.join("config.toml");
    let sidecar_path = session_dir.join(".codex-session-compose.json");

    let config_string = toml::to_string_pretty(&composition.merged_config).map_err(|err| {
        crate::config::ConfigError::MergeFailed { reason: err.to_string() }
    })?;
    write_atomic(&config_path, &config_string)?;

    // Emit profile sibling files 1:1 from configs/profiles/<name>.config.toml.
    for profile in &composition.profile_files {
        let sibling = session_dir.join(format!("{}.config.toml", profile.name));
        write_atomic(&sibling, &profile.raw_toml)?;
    }

    let sidecar = ComposeSidecar {
        manifest: composition.manifest_path.as_ref(),
        layers: &composition.layer_paths,
        env: &composition.env,
        baseline_projects: baseline_projects_as_json(composition.baseline_projects.as_ref())?,
        profiles: &composition.profile_files,
    };
    let sidecar_string = serde_json::to_string_pretty(&sidecar).map_err(|err| {
        crate::config::ConfigError::MergeFailed { reason: err.to_string() }
    })?;
    write_atomic(&sidecar_path, &sidecar_string)?;

    Ok(())
}
```

Update `ComposeSidecar` to carry `profiles: &'a [ProfileFileRef]`. Update
`StockComposeSidecar` to carry `profiles: Vec<ProfileFileRef>` (always empty in stock mode).

### Step 7: Rename manifest YAML field and add `profile-files:`

In `src/services/config_recipe/manifest.rs`:

```rust
pub(crate) struct Manifest {
    #[serde(rename = "config-layers")]
    pub(crate) config_layers: serde_yaml_ng::Value,
    #[serde(rename = "profile-files", default)]
    pub(crate) profile_files: Option<serde_yaml_ng::Value>,
}
```

Rename every consumer of `manifest.settings_layers` to `manifest.config_layers`. Update the
validator to also validate `profile_files` (when present): same name regex as
`config_layers`, non-empty array of strings, no duplicates.

Update the error variant text to mention the new field name.

### Step 8: Rename cache-bootstrap layer label

In `src/services/config_recipe/mod.rs::compose()`, change the cache-layer push from
`name: "settings".to_owned()` to `name: "configs".to_owned()`. This is a sidecar-observability
label; the on-disk file name is still `settings.toml` until round 03.

### Step 9: Update doctor messages (settings/ → configs/)

In `src/commands/doctor.rs`:

- Line ≈ 497: `"no settings/ directory"` → `"no configs/ directory"`. Include hint: "Create
  `<configs_dir>/` and add at least one layer file (e.g. `<configs_dir>/base.toml`)."
- Line ≈ 1000: `"create the missing layer file under settings/"` → `"... under configs/"`.

Do NOT add legacy-form detection here yet — round 03 expands doctor to surface the migration
hint when an old `settings/` dir or a layer carrying `[profiles.*]` is detected. This round
keeps doctor minimal.

### Step 10: Rename test helpers and add `write_profile_file`

In `tests/support/mod.rs`:

- `write_settings_layer(name, body)` → `write_config_layer(name, body)` (writes under
  `self.configs_dir().join("{name}.toml")`).
- `settings_dir()` → `configs_dir()` returning `self.config_home.join("codex-session/configs")`.
- `cache_settings_path()` stays named `cache_settings_path()` returning the same legacy file
  name for this round (round 03 renames both the helper and the file).
- New: `write_profile_file(&self, name: &str, body: &str) -> Utf8PathBuf` writing to
  `self.configs_dir().join("profiles").join(format!("{name}.config.toml"))`. Auto-create the
  `profiles/` subdir if missing.

### Step 11: Update composer tests

In `tests/config_recipe_composition.rs`:

- Rename existing call sites: `write_settings_layer` → `write_config_layer`,
  `settings-layers:` → `config-layers:`. Fix the YAML strings inline in test fixtures.
- Add new tests:
  1. `composer_emits_profile_sibling_files_one_to_one` — manifest with `profile-files: [deep,
    fast]`, two profile files written, assert session dir contains both
     `deep.config.toml` and `fast.config.toml` with content bit-equal to the input files.
  2. `composer_emits_all_profile_files_when_manifest_omits_list` — three profile files on disk,
     manifest omits `profile-files`, assert all three emitted, sorted by name in the sidecar
     `profiles` array.
  3. `composer_rejects_legacy_profile_selector_in_input_layer` — `configs/base.toml` containing
     `profile = "deep"`, assert `compose()` returns `ConfigError::LegacyProfileSyntax` with
     `location` containing "config layer `base.toml`".
  4. `composer_rejects_legacy_profiles_table_in_input_layer` — `configs/base.toml` containing
     `[profiles.deep]`, same expected error.
  5. `composer_rejects_legacy_profile_header_in_profile_file` —
     `configs/profiles/deep.config.toml` containing `[profiles.deep]` (user error mirroring the
     old shape inside the new file), same expected error.
  6. `composer_emits_clean_base_config_when_profiles_present` — assert emitted `config.toml`
     contains no `profile` key and no `profiles` table, even when profile files are present.

### Step 12: Update other test files

- `tests/config_recipe_errors.rs` — rename YAML fixtures and helper calls; add legacy-form
  cases for input layers.
- `tests/cmd_config_recipe_compose.rs`, `tests/cmd_config_recipe_show.rs`,
  `tests/cmd_config_recipe_list.rs` — rename path strings, YAML keys, and any visible CLI
  message text that names `settings/`. Add an assertion in `compose` that the rendered output
  lists the emitted profile sibling files.
- `tests/cmd_config_status.rs` — update the expected resolved-path output from
  `.../settings` to `.../configs`.
- `tests/cmd_doctor.rs` — update "no settings/" assertion text to "no configs/". DO NOT add
  legacy-detection-message assertions yet — round 03 owns that.

### Step 13: Verify the build + run gates

```bash
cd /workspaces/codex-session
just lint        # fmt-check + clippy-strict + print-ownership lint
just test-unit   # unit tests
just test-integration  # integration tests
```

Fix compile errors as they surface (the rename is compiler-driven so this is mechanical).
Investigate any failing test individually — most should be either (a) a rename that the editor
missed, or (b) a fixture that still uses the legacy YAML field name.

### Final Step: Update plan index

Update `.plan/01-todo/configs-rename-split-profiles/README.md`:

1. In the `## Execution Order` table, find the row for round 02.
2. Change `Status` from `todo` to `done`.
3. Change `Completed` from `--` to today's date (`YYYY-MM-DD`).

## Acceptance Criteria

- [ ] `ConfigError::LegacyProfileSyntax { location, reason }` variant exists, maps to exit code
      78 (`EX_CONFIG`), and surfaces the docs §F6c reference in its `Display` text.
- [ ] `layer::reject_legacy_profile_syntax` exists with unit-test coverage and is called from
      `compose()` on every input layer (base + profile files) AND from
      `write_session_artifacts` on the merged base config as a final bug net.
- [ ] `ConfigRecipeConfig`, `FileConfigRecipeConfig`, and `ConfigRecipePaths` use `configs_dir`
      (default value `config_dir.join("configs")`). No `settings_dir` field survives in
      `src/`.
- [ ] `Manifest` uses YAML field `config-layers:` (required) and `profile-files:` (optional).
      No `settings-layers:` references survive in `src/` or `tests/`.
- [ ] `Composition.profile_files: Vec<ProfileFileRef>` is populated by `compose()` from
      `configs/profiles/*.config.toml`.
- [ ] `write_session_artifacts` writes one `<name>.config.toml` per
      `Composition.profile_files` entry next to `config.toml`, bit-equal to the input file.
- [ ] Emitted `config.toml` contains no `profile = "..."` and no `[profiles.*]`. Verified by
      the new test `composer_emits_clean_base_config_when_profiles_present`.
- [ ] `ComposeSidecar.profiles` field lists each emitted profile file (name + source path).
- [ ] `tests/support/mod.rs` exposes `write_config_layer`, `configs_dir`, and
      `write_profile_file` helpers.
- [ ] All six new composer tests (step 11) pass.
- [ ] `just lint` passes.
- [ ] `just test` passes (unit + integration).
- [ ] `tests/account_health_cli.rs` is UNTOUCHED in this round (round 03 owns it).
- [ ] `src/services/account/gate.rs` `extract_ping_config` is UNTOUCHED beyond the
      `settings_dir` → `configs_dir` field rename (round 03 owns the heartbeat rewrite).
- [ ] Cache file `settings.toml` is still named `settings.toml` on disk and in the code
      (round 03 owns the rename).
- [ ] Plan `README.md` execution order table shows round 02 as `done` with today's date.

## Next Round

Round 03 — Heartbeat probe, cache rename, doctor — picks up with the composer emitting the
correct split layout. It rewrites `extract_ping_config` to read
`configs/profiles/ping.config.toml` directly, modifies `heartbeat_probe` to write a sibling
`ping.config.toml` under the probe `CODEX_HOME`, renames the cache file from `settings.toml`
to `configs.toml` (with trust-sync path updates and test fixture changes), and expands
doctor with explicit legacy-form detection that surfaces a one-line migration hint.
