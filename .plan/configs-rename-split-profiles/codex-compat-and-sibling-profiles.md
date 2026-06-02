# Round 04: Codex v0.134+ Fail-Fast Gate + Sibling `profiles/` Restructure

> Plan: configs-rename-split-profiles | Round: 04 of 05 | Complexity: L
> Generated: 2026-05-28 | Repos touched: `/workspaces/codex-session`

## Context

This round closes the wrapper-side gaps that rounds 01–03 left open, in two
ordered blocks confined to `/workspaces/codex-session`:

1. **Block A — Codex v0.134+ fail-fast gate.** Rounds 01–03 codified the
   contract (round 01), aligned the wrapper's emitted output to v0.134+'s
   input shape (round 02), and detected legacy _on-disk input_ forms (round
   03). What none of them check is the **installed `codex` binary's version
   itself.** If a user runs `codex-session …` against codex < 0.134.0, the
   wrapper happily emits the new sibling/no-profile-table shape and the child
   rejects it with its own legacy-form error — a confusing handoff that
   points at our emitted file instead of the version skew that caused it.
   Block A adds an explicit pre-launch gate (probes `codex --version`, parses
   it, refuses to launch if below 0.134.0) and mirrors the same check in
   `doctor` so users can diagnose the situation without running a
   pass-through.
2. **Block B — Sibling `profiles/` restructure.** Rounds 02 and 03 introduced
   and exercised the nested `configs/profiles/<name>.config.toml` layout as
   an intermediate. This block lifts `profiles/` to be a sibling of
   `configs/` — i.e. `~/.config/codex-session/profiles/<name>.config.toml`,
   not `.../configs/profiles/...` — by promoting `profiles_dir` to a
   first-class field defaulting to `config_dir.join("profiles")`. Every
   consumer, test fixture, in-repo doc paragraph, and doctor finding is
   updated; the doctor sweep gains a new detection for any surviving nested
   layout.

Block A lands before Block B: the gate is a contract-enforcement change
(it codifies _which_ upstream version the wrapper compiles against and
refuses to run against anything older), and the sibling restructure is a
layout shift downstream of that contract. Doing the gate first means every
subsequent test fixture in Block B can assume the new contract is in force.

The principle codified in round 01 (`CLAUDE.md` § Codex config compatibility)
drives every edit in both blocks. The cross-repo dotfiles propagation and
docs sync that previously lived in this round are now Round 05 — see
[`dotfiles-propagation-and-cross-repo-sync.md`](./dotfiles-propagation-and-cross-repo-sync.md).

## Previous Rounds

**Round 01 (expected state):** docs codify the API-compat principle. `README.md`
filesystem layout, `CLAUDE.md` § Codex config compatibility,
`docs/upstream-codex.md` §F6b–§F6c, and
`~/DocsNNotes/.../codex-conventions.md` describe the v0.134+ contract. The
filesystem layout in those docs uses the nested
`configs/profiles/<name>.config.toml` shape — block B rewrites every
nested-layout reference to the sibling form.

**Round 02 (expected state):** composer emits `<name>.config.toml` siblings
under `$CODEX_HOME/`. Manifest field is `config-layers:` + optional
`profile-files:`. The input layout is `configs/<layer>.toml` + nested
`configs/profiles/<name>.config.toml`. `ConfigRecipePaths::profiles_dir()` is
a derived method returning `configs_dir.join("profiles")`.
`Composition.profile_files: Vec<ProfileFileRef>` is populated and packed into
emitted output.

**Round 03 (expected state):** heartbeat probe reads the active recipe's ping
profile file via `composition.profile_files`; cache file renamed to
`configs.toml`; `ConfigRecipePaths.cache_config` replaces `cache_settings`;
doctor surfaces a one-line migration hint when legacy shapes are detected
(legacy `settings/` dir, legacy `profile = "..."` selector, legacy
`[profiles.*]` table). The nested `configs/profiles/<name>.config.toml`
layout is still in force — block B in this round flips it to sibling. **No
codex-binary version gate exists yet** — block A in this round adds it.

## Scope of This Round

### Block A — Codex v0.134+ fail-fast gate

**Rationale.** The wrapper's emitted `$CODEX_HOME/` tree is byte-for-byte
structurally compatible with codex v0.134+'s input contract (CLAUDE.md
§ Codex config compatibility). When the installed child binary is older than
0.134, that output shape is _invalid_ for the child — it will reject the
emitted file with its own legacy-form error pointing at
developers.openai.com. The wrapper must refuse to launch the child in that
case, with a message that names the version skew directly instead of letting
codex surface a misleading error about our emitted file.

This mirrors the existing fail-fast for "codex not installed" /
"codex not executable" (`AppError::ChildNotFound`,
`AppError::ChildNotExecutable` at `src/error.rs:25-39`) — one more rung on
the same gate: **present → executable → version compatible.**

**Where the check runs.** Two points, both fed from a single cached probe:

1. **Pre-launch, in every code path that invokes the child.** Today every
   such path resolves the binary via `AppContext::resolved_child()`
   (`src/context.rs:149-152`), which itself caches the result of
   `StdSpawner::resolve_child()` (`src/adapters/spawner.rs:161-211`). The
   version probe extends the existing `LazyChild` (`src/context.rs:20-42`)
   so the parsed version is cached alongside the resolved path. Pass-through
   (`src/commands/pass_through.rs:91-148`), account-health heartbeat (via
   `src/services/account/gate.rs::extract_ping_config`), account login /
   logout (`src/commands/pass_through.rs:44-47` → `gate::run_login` /
   `gate::run_logout`), and any other current-or-future codex-invoking path
   inherit the gate for free.
2. **In `doctor`** as a new check function `check_codex_version_minimum`,
   placed immediately after `check_child_binary`
   (`src/commands/doctor.rs:806`). Same `CheckResult` shape as the
   surrounding checks; status `Ok` when the parsed version's `major.minor`
   is at least `0.134`, `Fail` when below, `Warn` when the version string is
   unparsable. `doctor` continues to run end-to-end even when this finding
   is `Fail` — that's the value: a single `codex-session doctor` invocation
   surfaces the version skew without needing to attempt a pass-through.

"As early as possible" is interpreted as: as early as possible _on any code
path that depends on the v0.134+ contract_. Putting the gate in `main()`
before dispatch is **rejected** because it would block users from running
`codex-session doctor` to diagnose the very problem the gate detects, and
`codex-session --version` / `config-recipe list|show` (which don't invoke
codex) have no reason to fail. The chosen placement gates every
codex-invoking path without poisoning the diagnostic commands.

**Pre-release rule.** Any version with `major.minor >= 0.134` is `Ok`,
including pre-release suffixes (`0.134.0-rc1`, `0.134.0-alpha.1`, etc.).
Comparison floor is `0.134.0-0` (the lowest possible pre-release of
0.134.0) so `0.134.0-alpha.1 >= 0.134.0-0` evaluates true while
`0.133.99 >= 0.134.0-0` evaluates false. Versions below that floor are
`Fail`. Unparsable version strings are `Warn`, not `Fail` — the wrapper
proceeds against odd-but-likely-fine builds and surfaces the situation in
doctor.

**In scope (code & tests):**

- **New module `src/codex_compat.rs`** holding:
  - `pub const REQUIRED_CODEX_VERSION: &str = "0.134.0";` and an accessor
    `pub fn required_floor() -> &'static semver::Version` returning the
    parsed `0.134.0-0` floor (lazy-initialized via `OnceCell`).
  - `pub enum VersionCheck { Ok(semver::Version), TooOld(semver::Version),
  Unparsable(String) }`.
  - `pub fn classify(raw: &str) -> VersionCheck` — strips the leading
    `codex` / `codex-cli` token, trims, parses via
    `semver::Version::parse`, compares with `cmp_precedence` against
    `required_floor()`.
- **New error variants in `src/error.rs`**:
  ```rust
  ChildVersionTooOld { found: String, required: &'static str }
  ChildVersionUnparsable { raw: String }
  ```
  Both map to exit code **78** (config-error class — matches
  `Config(ConfigError)` at `src/error.rs:21-23` since this is a
  configuration-of-environment problem). `error_hint()` at
  `src/error.rs:586-689` gains hints citing
  `docs/upstream-codex.md §F6c` and naming the upgrade path
  (`codex --version` / install instructions). `Unparsable` is reserved
  for the rare case where the gate is called directly on an unparsable
  string (e.g. a forced check); the doctor path treats `Unparsable` as
  `Warn` and continues without raising the error.
- **Probe + parse**: extend `child_version_line` in
  `src/adapters/spawner.rs:213-244` with a sibling `child_version_parsed`
  that returns `VersionCheck`. The existing recursion guard (lines 219-234)
  stays in place — we still invoke `<child> --version` once per process.
- **`LazyChild` version caching**: add a `version: OnceCell<VersionCheck>`
  field next to the existing `path` field at `src/context.rs:20-42`. New
  accessor `AppContext::resolved_child_with_version_check()` returns
  `(ResolvedChild, &VersionCheck)`. The existing `resolved_child()` keeps
  its signature for callers that only need the path.
- **Gate method**: new `AppContext::ensure_child_version() -> Result<(),
  AppError>` short-circuits on the cached `OnceCell`. Returns
  `ChildVersionTooOld` for `TooOld`, `Ok(())` for both `Ok` and
  `Unparsable` (warn-not-fail — see pre-release rule above).
- **Pass-through wiring**: `pass_through::run()` calls
  `ctx.ensure_child_version()` immediately after the existing
  `resolved_child()` call at lines 632-656 in
  `src/commands/pass_through.rs`. Login / logout / health paths inherit the
  gate through the same `resolved_child()` plumbing.
- **Doctor wiring**: new `check_codex_version_minimum(ctx, &mut checks)` in
  `src/commands/doctor.rs`, invoked from `build_report()` (line 94)
  immediately after `check_child_binary` (line 806). Reuses the cached
  `OnceCell` so doctor doesn't re-fork codex.
- **Cargo dep**: add `semver = "1"` to `Cargo.toml` if not already present
  (executor verifies first).
- **Unit tests** (in `src/codex_compat.rs`):
  - `classify("codex 0.134.0\n")` → `Ok`.
  - `classify("codex-cli 0.134.0\n")` → `Ok` (parser strips both prefixes).
  - `classify("  codex 0.134.0  \n")` → `Ok` (whitespace tolerated).
  - `classify("codex 0.134.0-rc1")` → `Ok` (pre-release of 0.134.0 passes).
  - `classify("codex 0.134.0-alpha.1")` → `Ok`.
  - `classify("codex 0.135.0")` → `Ok`.
  - `classify("codex 1.0.0")` → `Ok`.
  - `classify("codex 0.133.99")` → `TooOld`.
  - `classify("codex 0.0.1")` → `TooOld`.
  - `classify("garbage output")` → `Unparsable`.
- **Integration tests** (in `tests/`, exact module location follows the
  existing pattern — likely `tests/cmd_pass_through.rs` and
  `tests/cmd_doctor.rs`):
  - `pass_through_fails_fast_on_old_codex` — `MockSpawner` returns
    `codex 0.133.0` for `--version`; the wrapper exits **78** before any
    other child invocation, stderr cites
    `docs/upstream-codex.md §F6c`.
  - `doctor_reports_codex_version_too_old` — same mock; doctor prints a
    `FAIL` finding for the new check and still exits 1 (existing behavior:
    `summary.fail > 0`).
  - `doctor_reports_codex_version_ok` — mock returns `codex 0.134.0`;
    doctor's new check is `OK`.
  - `doctor_warns_on_unparsable_codex_version` — mock returns
    `weird-output\n`; doctor's new check is `Warn`; exit code unaffected.

**Out of scope (Block A):**

- Editing `docs/upstream-codex.md`. §F6c (round 01) already names the
  v0.134.0 contract; the new error hints just link to it. No doc change
  needed.
- Adding a CLI flag to bypass the gate. The gate is unconditional —
  consistent with the existing `ChildNotFound` / `ChildNotExecutable`
  gates which also have no opt-out. Users who need to test against an
  older codex use the `MockSpawner` integration tests.
- Re-probing the version on every child invocation. The
  `OnceCell` cache makes the gate free on the second-and-later call.

### Block B — Sibling `profiles/` restructure

**In scope (code & tests):**

- `src/config/mod.rs::ConfigRecipeConfig`: add `profiles_dir: Utf8PathBuf`
  field next to `configs_dir`. Default constructor:
  `profiles_dir: config_dir.join("profiles")` (sibling).
- `src/config/mod.rs::FileConfigRecipeConfig`: add
  `profiles_dir: Option<Utf8PathBuf>` with
  `#[serde(rename = "profiles-dir")]`. Resolver merges into the runtime
  config like `configs_dir`.
- `src/services/config_recipe/composition.rs::ConfigRecipePaths`: add
  `profiles_dir: Utf8PathBuf` as a first-class field; delete the derived
  `profiles_dir()` method.
- `src/services/config_recipe/mod.rs::compose()`: pass `paths.profiles_dir`
  (the field, not a derived join). When the manifest's `profile-files:` is
  omitted, scan `paths.profiles_dir` (sibling) instead of
  `paths.configs_dir.join("profiles")`.
- `src/services/account/gate.rs`: every `ConfigRecipePaths { … }`
  constructor adds `profiles_dir:
  ctx.config.config_recipe.profiles_dir.clone()`.
- `src/commands/{pass_through.rs, doctor.rs, …}` and any other constructor
  of `ConfigRecipePaths`: same field addition. Compiler will surface them
  all.
- `src/commands/doctor.rs::check_legacy_profile_forms` (added in round 03):
  widen the sweep to also flag the obsolete nested layout — if
  `paths.configs_dir.join("profiles").is_dir()` AND it is not the same
  path as `paths.profiles_dir`, emit a warning pointing at the new sibling
  layout. Continue to read profile files from `paths.profiles_dir` for the
  legacy-syntax sweep.
- Update doctor's no-profiles-dir messaging (if any) and the comment trail
  in `composition.rs` / `layer.rs` that names
  `configs/profiles/<name>.config.toml` — switch to
  `profiles/<name>.config.toml`.
- `README.md` (lines 87, 110): rewrite the filesystem-layout block and the
  "where do profile overrides live?" prose to describe sibling
  `profiles/<name>.config.toml`.
- `docs/upstream-codex.md` (lines 147, 214): same edit — rewrite both
  paragraphs to name the sibling path.
- `CLAUDE.md` § Codex config compatibility: the rule itself does not name a
  path, but the example in the bullet about sibling files stays (already
  correct: emitted output is `$CODEX_HOME/<name>.config.toml` siblings —
  that's the _output_ shape, unchanged by this restructure). Verify and add
  one clarifying parenthetical that the _input_ layout in
  `$XDG_CONFIG_HOME/codex-session/` also uses sibling `configs/` and
  `profiles/` dirs.
- `tests/support/mod.rs`: `write_profile_file(name, body)` now writes to
  `self.profiles_dir().join(format!("{name}.config.toml"))`; add
  `profiles_dir()` helper returning
  `self.config_home.join("codex-session/profiles")`.
- All composer, account-health, doctor, and config-recipe tests that wrote
  profile files under the nested path are recompiled by the helper rename
  — fix any fixture string that still hardcodes `configs/profiles/`.
- Add a new doctor test:
  `doctor_warns_on_nested_configs_profiles_dir` — create
  `<config>/configs/profiles/` on disk alongside `<config>/profiles/`,
  assert doctor stderr contains the migration hint pointing at the new
  sibling layout.

**Out of scope (Block B):**

- Dotfiles repo migration, dotfiles cleanup, cross-repo docs sweep. Those
  are Round 05.
- Renaming or reshaping `configs_dir` itself. Round 02 already named it
  `configs_dir`; this block adds `profiles_dir` next to it without
  touching the existing field.

## Current State

### Block A (codex-binary version gate)

- `child_version_line` exists at `src/adapters/spawner.rs:213-244` and is
  used only by `src/commands/version.rs:40-42` for display. No gate, no
  parsing, no caching.
- `LazyChild` at `src/context.rs:20-42` caches the resolved binary path
  but not the version.
- `pass_through::run()` resolves the binary at lines 632-656 of
  `src/commands/pass_through.rs` via `ctx.resolved_child()` and proceeds
  directly to spawn; no version check between the two.
- `src/commands/doctor.rs:806::check_child_binary` reports the version
  string verbatim but never compares it against a floor.
- `src/error.rs` defines `ChildNotFound` (exit 127),
  `ChildNotExecutable` (exit 126), `ChildExec` (exit 74),
  `ChildRecursion` (exit 70) — no version-related variants.
- `Cargo.toml` may or may not already depend on `semver` — executor
  verifies in step A1.

### Block B (sibling `profiles/` restructure)

- `src/config/mod.rs::ConfigRecipeConfig` has `configs_dir: Utf8PathBuf`
  (added in round 02). No `profiles_dir` field yet — the profile dir is
  derived in `ConfigRecipePaths::profiles_dir()`.
- `src/services/config_recipe/composition.rs::ConfigRecipePaths` exposes
  `profiles_dir()` as a method returning `configs_dir.join("profiles")`
  (round 02).
- `src/services/config_recipe/mod.rs::compose()` calls
  `paths.profiles_dir()` when scanning the manifest-omits-list case
  (round 02).
- `src/commands/doctor.rs::check_legacy_profile_forms` (added in round 03)
  sweeps `configs_dir.join("profiles")` for legacy-syntax findings. It
  does NOT yet detect a nested `configs/profiles/` dir as a
  layout-migration finding.
- `README.md` filesystem layout (lines ~80–90) describes
  `configs/profiles/<name>.config.toml`.
- `README.md` "user-config" prose (line ~110) names
  `configs/profiles/<name>.config.toml`.
- `docs/upstream-codex.md` line 147 (`Mitigation in codex-session`) names
  `configs/profiles/ping.config.toml`.
- `docs/upstream-codex.md` line 214 (`§F6c Adjacent invariants`) names
  `configs/profiles/*.config.toml`.
- `tests/support/mod.rs::write_profile_file` writes under
  `self.configs_dir().join("profiles")`.

### Existing Patterns

- Rust field rename / addition is compiler-driven: add the field, fix
  every constructor the compiler points at. Keep the `profiles_dir` field
  next to `configs_dir` in every struct for readability.
- New module / new error variant lands as a compiler-friendly addition:
  add the variant, the match arms surface, add hints in `error_hint()`,
  thread the new field through any `Display` impl.
- `MockSpawner` (referenced for integration tests) — executor locates the
  existing mock under `tests/support/` and extends its
  `child_version_line` response in step A7.

## Implementation Steps

### Block A — Codex v0.134+ fail-fast gate

#### A1. Verify `semver` is available; add the new `codex_compat` module

```bash
grep -E '^semver' Cargo.toml || echo "needs semver dep"
```

If missing, add `semver = "1"` under `[dependencies]`. Create
`src/codex_compat.rs` with the `REQUIRED_CODEX_VERSION` constant,
`required_floor()` accessor, `VersionCheck` enum, and `classify(raw: &str)`
function per the design above. Register the module in `src/lib.rs` (or
`src/main.rs`, whichever owns the crate's module tree).

#### A2. Add the new error variants

In `src/error.rs`:

```rust
#[error("codex CLI version {found} is too old; codex-session requires >= {required}. \
        See docs/upstream-codex.md §F6c.")]
ChildVersionTooOld { found: String, required: &'static str },

#[error("codex CLI version output not parseable: {raw}. See docs/upstream-codex.md §F6c.")]
ChildVersionUnparsable { raw: String },
```

Both map to exit code 78 in `exit_code()`. In `error_hint()` (around lines
586-689) add hints citing the docs section and naming the upgrade path
(install instructions, `CODEX_SESSION_CHILD_BIN` override for
contained-environment testing).

#### A3. Extend `LazyChild` with version caching

In `src/context.rs`, add a `version: OnceCell<VersionCheck>` field next to
`path`. Add `AppContext::resolved_child_with_version_check()` returning
`(ResolvedChild, &VersionCheck)`. Add
`AppContext::ensure_child_version() -> Result<(), AppError>`. The latter
runs the cached classifier, returns `Err(ChildVersionTooOld)` for `TooOld`,
`Ok(())` for `Ok` and `Unparsable`.

#### A4. Wire the gate into pass-through dispatch

In `src/commands/pass_through.rs`, immediately after the existing
`resolved_child()` call at lines 632-656, insert
`ctx.ensure_child_version()?;`. Verify by reading the surrounding code that
no other code path can reach a `Spawner::spawn` without going through this
gate — list every `spawn` / `Command::new` call in `src/` and confirm.

#### A5. Wire the new check into doctor

In `src/commands/doctor.rs::build_report()` (around line 94), after the
existing `check_child_binary` call at line 806, push a new
`check_codex_version_minimum(ctx, &mut checks)` that:

- Calls `ctx.resolved_child_with_version_check()` (re-using the cache).
- Maps `Ok(v)` → `CheckResult::ok("codex.version", format!("{v} (>= {})",
  REQUIRED_CODEX_VERSION))`.
- Maps `TooOld(v)` → `CheckResult::fail("codex.version", …)` with a detail
  string naming the required floor and pointing at
  `docs/upstream-codex.md §F6c`.
- Maps `Unparsable(raw)` → `CheckResult::warn("codex.version", …)` with
  detail naming the raw output.

#### A6. Unit tests for `codex_compat::classify`

Add a `#[cfg(test)] mod tests` block at the bottom of `src/codex_compat.rs`
covering every case enumerated in the **Tests** subsection of Block A's
scope above.

#### A7. Integration tests for the gate and doctor finding

Extend `MockSpawner` (or its equivalent in `tests/support/`) so its
`child_version_line` response is configurable per-test. Add the four
integration tests enumerated in Block A's scope:
`pass_through_fails_fast_on_old_codex`,
`doctor_reports_codex_version_too_old`, `doctor_reports_codex_version_ok`,
`doctor_warns_on_unparsable_codex_version`.

#### A8. Doc updates (no-op verification)

Verify that `docs/upstream-codex.md §F6c` already names the v0.134.0
contract (round 01). Confirm no doc edit is needed in Block A — the new
error hints reference §F6c verbatim, the section already exists.

#### A9. Run gates

```bash
cd /workspaces/codex-session
just lint
just test
just precommit-all
```

#### A10. Stage Block A changes

Do NOT commit yet — Block B is part of the same round and lands in the
same commit. Verify staging:

```bash
git status --short
git diff --stat
```

### Block B — Sibling `profiles/` restructure

#### B1. Add `profiles_dir` to `ConfigRecipeConfig` and `FileConfigRecipeConfig`

In `src/config/mod.rs`:

```rust
pub struct ConfigRecipeConfig {
  pub default: Option<String>,
  pub config_dir: Utf8PathBuf,
  pub recipes_dir: Utf8PathBuf,
  pub configs_dir: Utf8PathBuf,
  pub profiles_dir: Utf8PathBuf,   // NEW — sibling of configs_dir
  pub active: Option<String>,
}
```

In the default constructor near lines 276–282 (after `configs_dir`):

```rust
profiles_dir: config_dir.join("profiles"),
```

In `FileConfigRecipeConfig`:

```rust
#[serde(rename = "profiles-dir", default)]
pub profiles_dir: Option<Utf8PathBuf>,
```

Update the file→runtime config resolver (grep for `configs_dir:` to find
the merge site) to thread `profiles_dir` through the same way.

#### B2. Promote `profiles_dir` to a field in `ConfigRecipePaths`

In `src/services/config_recipe/composition.rs`:

```rust
pub(crate) struct ConfigRecipePaths {
  pub(crate) recipes_dir: Utf8PathBuf,
  pub(crate) configs_dir: Utf8PathBuf,
  pub(crate) profiles_dir: Utf8PathBuf,  // NEW first-class field
  pub(crate) cache_config: Option<Utf8PathBuf>,
}
```

Delete the `impl ConfigRecipePaths { fn profiles_dir(...) }` derived-method
block from round 02. The compiler will surface every call site.

#### B3. Update every `ConfigRecipePaths { … }` constructor

Grep for `ConfigRecipePaths {` across `src/`. Each constructor (in
`mod.rs::compose` callers, `gate.rs::extract_ping_config`, command
modules) gains:

```rust
profiles_dir: ctx.config.config_recipe.profiles_dir.clone(),
```

next to the `configs_dir` line.

#### B4. Update `compose()` to use `paths.profiles_dir` directly

In `src/services/config_recipe/mod.rs::compose()`, replace
`paths.profiles_dir()` (method call) with `paths.profiles_dir.clone()` or
`&paths.profiles_dir` (field access) at every call site. The directory-scan
and the manifest-declared-name path-join both use `paths.profiles_dir` as
the parent dir.

#### B5. Widen the doctor sweep to flag the obsolete nested layout

In `src/commands/doctor.rs::check_legacy_profile_forms`, after the existing
legacy-form checks, add:

```rust
let nested_profiles = ctx.config.config_recipe.configs_dir.join("profiles");
if nested_profiles.is_dir() && nested_profiles != ctx.config.config_recipe.profiles_dir {
  diagnostics.push_warning(format!(
      "obsolete nested `{nested_profiles}` directory detected. Profile files \
      moved to a sibling layout in round 04 of the configs-rename-split-profiles \
      plan; move per-profile files to `{}` (sibling of `configs/`). \
      See docs/upstream-codex.md §F6c.",
      ctx.config.config_recipe.profiles_dir
  ));
}
```

Also update the `sweep_dir_for_legacy` call site that previously passed
`configs_dir.join("profiles")` — switch it to
`&ctx.config.config_recipe.profiles_dir`.

#### B6. Update in-repo prose docs

`/workspaces/codex-session/README.md`:

- Line ~87 (filesystem-layout block): rewrite to show
  `~/.config/codex-session/configs/<layer>.toml` and a sibling
  `~/.config/codex-session/profiles/<name>.config.toml` rather than the
  nested form.
- Line ~110 (user-config prose explaining where overrides live): same edit
  — `profiles/` is a sibling of `configs/`, not a child.

`/workspaces/codex-session/docs/upstream-codex.md`:

- Line 147 (Mitigation in codex-session): rewrite
  `configs/profiles/ping.config.toml` → `profiles/ping.config.toml`. Adjust
  surrounding prose if it explicitly says "nested".
- Line 214 (§F6c Adjacent invariants): rewrite
  `configs/profiles/*.config.toml` → `profiles/*.config.toml`.
- Bump the `Last verified` date at the top of `docs/upstream-codex.md` to
  today.

`/workspaces/codex-session/CLAUDE.md`:

- § Codex config compatibility: the bullet about emitted output is correct
  as-is (the emitted shape `$CODEX_HOME/<name>.config.toml` siblings does
  not change). Add one parenthetical to the input-layer bullet clarifying
  that input layers live under `configs/` and per-profile overrides under
  sibling `profiles/`. Keep the edit minimal.

#### B7. Update `tests/support/mod.rs`

```rust
pub fn profiles_dir(&self) -> Utf8PathBuf {
  self.config_home.join("codex-session/profiles")
}

pub fn write_profile_file(&self, name: &str, body: &str) -> Utf8PathBuf {
  let dir = self.profiles_dir();
  std::fs::create_dir_all(dir.as_std_path()).unwrap();
  let path = dir.join(format!("{name}.config.toml"));
  std::fs::write(path.as_std_path(), body).unwrap();
  path
}
```

Delete any remaining `configs_dir().join("profiles")` joins in the helpers.

#### B8. Fix test fixtures that hardcoded the nested path

Grep `tests/` for `configs/profiles` and `configs_dir().join("profiles")` —
there should be zero matches after `write_profile_file` is updated, but
inline string-literal fixtures (e.g. doctor stderr assertions,
config-status output assertions) may still hardcode the nested path.
Update each to the sibling form.

#### B9. Add doctor test for the new nested-layout finding

In `tests/cmd_doctor.rs`, add:

```rust
#[test]
fn doctor_warns_on_nested_configs_profiles_dir() {
  let env = TestEnv::new();
  // Create the obsolete nested dir.
  let nested = env.configs_dir().join("profiles");
  std::fs::create_dir_all(nested.as_std_path()).unwrap();
  // Also create the new sibling dir so the only finding is "obsolete nested".
  std::fs::create_dir_all(env.profiles_dir().as_std_path()).unwrap();
  env.write_config_layer("base", "");

  env.command()
      .args(["doctor"])
      .assert()
      .stderr(predicate::str::contains("obsolete nested"))
      .stderr(predicate::str::contains("profiles/"))
      .stderr(predicate::str::contains("docs/upstream-codex.md §F6c"));
}
```

#### B10. Run gates and commit

```bash
cd /workspaces/codex-session
just lint
just test
just precommit-all
```

Stage and commit Block A + Block B together (single round, single commit):

```bash
git add src/ tests/ Cargo.toml Cargo.lock README.md CLAUDE.md \
      docs/upstream-codex.md .plan/01-todo/configs-rename-split-profiles/

git commit -m "$(cat <<'EOF'
Feat(config): codex v0.134+ gate and sibling `profiles/` layout (round 04)

Rounds 01-03 codified and emitted the codex v0.134+ contract but never
verified the installed `codex` binary itself was >= 0.134. This round
closes both remaining wrapper-side gaps.

Block A — codex version fail-fast:
- New `src/codex_compat.rs` exposing `REQUIRED_CODEX_VERSION = 0.134.0`,
  a `VersionCheck` enum, and a `classify` parser tolerating
  `codex` / `codex-cli` prefixes and pre-release suffixes.
- New error variants `AppError::ChildVersionTooOld` and
  `ChildVersionUnparsable` (exit 78) with hints citing
  `docs/upstream-codex.md §F6c`.
- `LazyChild` caches the parsed version;
  `AppContext::ensure_child_version()` short-circuits on the cache and
  is called from `pass_through::run()` before any child invocation.
  Account health, login, logout inherit the gate via the same plumbing.
- New `doctor::check_codex_version_minimum` mirrors the gate as a
  surfaced check (Ok / Fail / Warn).

Block B — sibling `profiles/` restructure:
- `profiles_dir` is promoted from a derived
  `configs_dir.join("profiles")` method to a first-class field on
  `ConfigRecipeConfig`, `FileConfigRecipeConfig`, and
  `ConfigRecipePaths` defaulting to `config_dir.join("profiles")`. The
  wrapper's input layout is now `configs/<layer>.toml` + sibling
  `profiles/<name>.config.toml`.
- Doctor sweep gains a finding for any surviving nested
  `configs/profiles/` directory.
- README, docs/upstream-codex.md, and CLAUDE.md are updated to
  describe the sibling layout.

References: docs/upstream-codex.md §F6b-§F6c, CLAUDE.md § Codex config
compatibility.
EOF
)"
```

### Final Step: Update plan index

Update `.plan/01-todo/configs-rename-split-profiles/README.md` (in
`/workspaces/codex-session`):

1. In the `## Execution Order` table, find the row for round 04.
2. Change `Status` from `todo` to `done`.
3. Change `Completed` from `--` to today's date (`YYYY-MM-DD`).

The plan directory move (from `01-todo/` to `02-done/`) happens in Round
05's Final Step, after the cross-repo work is also complete.

## Acceptance Criteria

### Block A — codex v0.134+ fail-fast

- [ ] `src/codex_compat.rs` exists with `REQUIRED_CODEX_VERSION = "0.134.0"`,
      a `VersionCheck` enum, and a `classify(raw: &str) -> VersionCheck`
      parser tolerating `codex` / `codex-cli` prefixes and pre-release
      suffixes. Unit tests cover every case listed in the Block A scope.
- [ ] `AppError::ChildVersionTooOld { found, required }` and
      `AppError::ChildVersionUnparsable { raw }` variants exist with exit
      code **78**; `error_hint()` for both names
      `docs/upstream-codex.md §F6c` and the upgrade path.
- [ ] `LazyChild` caches the parsed version via `OnceCell`. Second-and-later
      calls within a process do not re-invoke `codex --version`.
- [ ] `AppContext::ensure_child_version()` short-circuits on the cache,
      returns `Err(ChildVersionTooOld)` for `TooOld`, `Ok(())` for `Ok`
      and `Unparsable`.
- [ ] `pass_through::run()` calls `ensure_child_version()` before any
      child invocation. Account health, login, logout inherit the gate
      via the same `resolved_child()` plumbing — verified by tracing
      every `Spawner::spawn` / `Command::new` call site in `src/`.
- [ ] `doctor::check_codex_version_minimum` exists, runs after
      `check_child_binary` (`src/commands/doctor.rs:806`), surfaces
      `Ok` / `Fail` / `Warn` per spec. `doctor` continues running other
      checks even when this finding is `Fail`.
- [ ] Integration tests exist and pass:
      `pass_through_fails_fast_on_old_codex`,
      `doctor_reports_codex_version_too_old`,
      `doctor_reports_codex_version_ok`,
      `doctor_warns_on_unparsable_codex_version`.
- [ ] Pre-release rule verified: `0.134.0-rc1`, `0.134.0-alpha.1`, and
      `0.134.0` all parse as `Ok`; `0.133.99` parses as `TooOld`;
      `garbage` parses as `Unparsable`.

### Block B — sibling `profiles/` restructure

- [ ] `ConfigRecipeConfig.profiles_dir: Utf8PathBuf` exists; default
      constructor sets it to `config_dir.join("profiles")`.
- [ ] `FileConfigRecipeConfig.profiles_dir: Option<Utf8PathBuf>` exists
      with `#[serde(rename = "profiles-dir", default)]` and is merged into
      the runtime config next to `configs_dir`.
- [ ] `ConfigRecipePaths.profiles_dir: Utf8PathBuf` is a first-class
      field; the derived `profiles_dir()` method from round 02 is deleted.
- [ ] Every `ConfigRecipePaths { … }` constructor in `src/` passes
      `profiles_dir`.
- [ ] `compose()` in `src/services/config_recipe/mod.rs` scans
      `paths.profiles_dir` (field, not `configs_dir.join("profiles")`).
- [ ] `doctor::check_legacy_profile_forms` emits a warning when an
      obsolete nested `<configs_dir>/profiles/` dir exists and differs
      from `<profiles_dir>`. Warning cites `docs/upstream-codex.md §F6c`
      and names the sibling target path.
- [ ] `README.md` filesystem-layout and prose sections describe sibling
      `profiles/<name>.config.toml`. No `configs/profiles/` substring
      survives in `README.md`.
- [ ] `docs/upstream-codex.md` lines previously naming
      `configs/profiles/...` now name sibling `profiles/...`.
      `Last verified` date bumped to today.
- [ ] `CLAUDE.md` § Codex config compatibility input-layer bullet
      clarifies sibling layout.
- [ ] `tests/support/mod.rs` exposes `profiles_dir()` and
      `write_profile_file(name, body)` rooted at
      `<config_home>/codex-session/profiles/`.
- [ ] `grep -rn "configs/profiles" src/ tests/ README.md CLAUDE.md docs/`
      returns no matches.
- [ ] New doctor test `doctor_warns_on_nested_configs_profiles_dir` exists
      and passes.

### Round-level

- [ ] `just lint`, `just test`, and `just precommit-all` all pass.
- [ ] One commit in `/workspaces/codex-session` captures Block A + Block B
      with a Conventional Commit message citing round 04 and naming both
      blocks.
- [ ] Plan `README.md` execution order table shows round 04 as `done`
      with today's date.

## Next Round

Round 05 — dotfiles propagation (`~/.dotfiles/codex-session`), dotfiles
cleanup, and cross-repo docs sync (`~/DocsNNotes`,
`~/.dotfiles/{claude,claude-session}`). See
[`dotfiles-propagation-and-cross-repo-sync.md`](./dotfiles-propagation-and-cross-repo-sync.md).
