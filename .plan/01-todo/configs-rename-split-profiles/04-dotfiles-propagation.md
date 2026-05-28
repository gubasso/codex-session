# Round 04: Sibling `profiles/` Restructure + Dotfiles Propagation + Cleanup + Cross-Repo Docs Sync

> Plan: configs-rename-split-profiles | Round: 04 of 04 | Complexity: XL
> Generated: 2026-05-28 | Repos touched: `/workspaces/codex-session`,
> `~/.dotfiles/codex-session`, `~/DocsNNotes`, `~/.dotfiles/claude`,
> `~/.dotfiles/claude-session`

## Context

This round finishes the configs-rename-split-profiles plan in one prex session, in four
ordered blocks:

1. **Block A — Sibling `profiles/` restructure** (in `/workspaces/codex-session`): rounds 02
    and 03 introduced and exercised the nested `configs/profiles/<name>.config.toml` layout as
    an intermediate. This block lifts `profiles/` to be a sibling of `configs/` — i.e.
    `~/.config/codex-session/profiles/<name>.config.toml`, not `.../configs/profiles/...` — by
    promoting `profiles_dir` to a first-class field defaulting to
    `config_dir.join("profiles")`. Every consumer, test fixture, in-repo doc paragraph, and
    doctor finding gets updated; the doctor sweep gains a new detection for any surviving
    nested layout.
2. **Block B — Dotfiles propagation** (in `~/.dotfiles/codex-session`): migrate the
    stow-managed source-of-truth to the final sibling layout. Rename `settings/` → `configs/`,
    extract `[profiles.{deep,fast,ping}]` blocks into the new sibling
    `profiles/<name>.config.toml` files, update `default.yaml`'s manifest key
    `settings-layers:` → `config-layers:`.
3. **Block C — Dotfiles cleanup** (in `~/.dotfiles/codex-session`): remove obsolete leftovers
    from prior plan iterations (an existing `settings.bak.20260528/` backup and a
    half-migrated `configs/` tree alongside the legacy `settings/`) and prune anything no
    longer relevant under the new sibling layout. Keep the repo lean and on-par with the
    final wrapper implementation.
4. **Block D — Cross-repo docs sync**: sweep `~/DocsNNotes` and
    `~/.dotfiles/{codex-session,claude,claude-session}` for codex-session-relevant docs that
    still describe pre-v0.134 / nested-layout / `settings/`-vocabulary state and update each
    to match. Scope is strictly **codex-session-relevant content only** — unrelated
    Claude/Claude-session config files and skill bundles are not touched.

Each affected repo gets its own commit. The four repos are independent git repos; the
executor must `cd` into each one explicitly and not cross commit boundaries.

The principle codified in round 01 (`CLAUDE.md` § Codex config compatibility) drives every
edit in every block.

## Previous Rounds

**Round 01 (expected state):** docs codify the API-compat principle. `README.md` filesystem
layout, `CLAUDE.md` § Codex config compatibility, `docs/upstream-codex.md` §F6b–§F6c, and
`~/DocsNNotes/.../codex-conventions.md` describe the v0.134+ contract. The filesystem
layout in those docs uses the nested `configs/profiles/<name>.config.toml` shape — block A
rewrites every nested-layout reference to the sibling form.

**Round 02 (expected state):** composer emits `<name>.config.toml` siblings under
`$CODEX_HOME/`. Manifest field is `config-layers:` + optional `profile-files:`. The input
layout is `configs/<layer>.toml` + nested `configs/profiles/<name>.config.toml`.
`ConfigRecipePaths::profiles_dir()` is a derived method returning
`configs_dir.join("profiles")`. `Composition.profile_files: Vec<ProfileFileRef>` is populated
and packed into emitted output.

**Round 03 (expected state):** heartbeat probe reads the active recipe's ping profile file
via `composition.profile_files`; cache file renamed to `configs.toml`;
`ConfigRecipePaths.cache_config` replaces `cache_settings`; doctor surfaces a one-line
migration hint when legacy shapes are detected (legacy `settings/` dir, legacy
`profile = "..."` selector, legacy `[profiles.*]` table). The nested
`configs/profiles/<name>.config.toml` layout is still in force — block A in this round
flips it to sibling.

## Scope of This Round

**In scope (Block A — `/workspaces/codex-session`):**

- `src/config/mod.rs::ConfigRecipeConfig`: add `profiles_dir: Utf8PathBuf` field next to
  `configs_dir`. Default constructor: `profiles_dir: config_dir.join("profiles")` (sibling).
- `src/config/mod.rs::FileConfigRecipeConfig`: add `profiles_dir: Option<Utf8PathBuf>` with
  `#[serde(rename = "profiles-dir")]`. Resolver merges into the runtime config like
  `configs_dir`.
- `src/services/config_recipe/composition.rs::ConfigRecipePaths`: add
  `profiles_dir: Utf8PathBuf` as a first-class field; delete the derived
  `profiles_dir()` method.
- `src/services/config_recipe/mod.rs::compose()`: pass `paths.profiles_dir` (the field, not a
  derived join). When the manifest's `profile-files:` is omitted, scan
  `paths.profiles_dir` (sibling) instead of `paths.configs_dir.join("profiles")`.
- `src/services/account/gate.rs`: every `ConfigRecipePaths { … }` constructor adds
  `profiles_dir: ctx.config.config_recipe.profiles_dir.clone()`.
- `src/commands/{pass_through.rs, doctor.rs, …}` and any other constructor of
  `ConfigRecipePaths`: same field addition. Compiler will surface them all.
- `src/commands/doctor.rs::check_legacy_profile_forms` (added in round 03): widen the sweep
  to also flag the obsolete nested layout — if `paths.configs_dir.join("profiles").is_dir()`
  AND it is not the same path as `paths.profiles_dir`, emit a warning pointing at the new
  sibling layout. Continue to read profile files from `paths.profiles_dir` for the
  legacy-syntax sweep.
- Update doctor's no-profiles-dir messaging (if any) and the comment trail in
  `composition.rs` / `layer.rs` that names `configs/profiles/<name>.config.toml` — switch
  to `profiles/<name>.config.toml`.
- `README.md` (lines 87, 110): rewrite the filesystem-layout block and the "where do
  profile overrides live?" prose to describe sibling `profiles/<name>.config.toml`.
- `docs/upstream-codex.md` (lines 147, 214): same edit — rewrite both paragraphs to name
  the sibling path.
- `CLAUDE.md` § Codex config compatibility: the rule itself does not name a path, but the
  example in the bullet about sibling files stays (already correct: emitted output is
  `$CODEX_HOME/<name>.config.toml` siblings — that's the *output* shape, unchanged by this
  restructure). Verify and add one clarifying parenthetical that the *input* layout in
  `$XDG_CONFIG_HOME/codex-session/` also uses sibling `configs/` and `profiles/` dirs.
- `tests/support/mod.rs`: `write_profile_file(name, body)` now writes to
  `self.profiles_dir().join(format!("{name}.config.toml"))`; add `profiles_dir()` helper
  returning `self.config_home.join("codex-session/profiles")`.
- All composer, account-health, doctor, and config-recipe tests that wrote profile files
  under the nested path are recompiled by the helper rename — fix any fixture string that
  still hardcodes `configs/profiles/`.
- Add a new doctor test: `doctor_warns_on_nested_configs_profiles_dir` — create
  `<config>/configs/profiles/` on disk alongside `<config>/profiles/`, assert doctor stderr
  contains the migration hint pointing at the new sibling layout.
- Run `just lint` and `just test` to confirm the restructure compiles and passes.
- Commit in `/workspaces/codex-session` with a Conventional Commit citing the round.

**In scope (Block B — `~/.dotfiles/codex-session`):**

- Rename `.config/codex-session/settings/` → `.config/codex-session/configs/` (via
  `git mv`) — but note that `.config/codex-session/configs/` already exists on disk from an
  earlier ad-hoc migration. Block C handles the reconciliation between the two; block B
  authors the *final* target state for `configs/` (base config layers, no profile keys).
- Author the final `configs/base.toml` content: strip `profile = "deep"` and the
  `[profiles.{deep,fast,ping}]` tables. Keep `web_search` and `[features]`.
- Author the final `configs/plugins.toml` and `configs/projects.toml` content: identical to
  the current `settings/` versions (no profile keys to extract).
- Create `.config/codex-session/profiles/` (sibling of `configs/`) with
  `deep.config.toml`, `fast.config.toml`, `ping.config.toml` — bare top-level keys, no
  `[profiles.<name>]` header.
- Update `.config/codex-session/config-recipes/default.yaml`: rename
  `settings-layers:` → `config-layers:`. Do not add `profile-files:` (omit-and-emit-all
  matches the user's prior intent: all three profiles available, none auto-activated).
- Update the comment trail in `.config/codex-session/config.toml` to reference `configs/`
  and `profiles/` (sibling) — drop the old `settings/` mention.

**In scope (Block C — `~/.dotfiles/codex-session` cleanup):**

- Delete `.config/codex-session/settings.bak.20260528/` (entire dir) — leftover from a
  prior ad-hoc migration; obsolete under the new layout.
- Delete `.config/codex-session/settings/` (the legacy stow source) — replaced by
  `configs/` (base layers) + `profiles/` (overrides).
- Review every other file under `~/.dotfiles/codex-session/.config/codex-session/` and the
  repo root (excluding `.agents/skills/` — that's an unrelated bundle): remove anything
  that references the old `settings/` vocabulary, the legacy `[profiles.*]` shape, the
  nested `configs/profiles/` path, or is otherwise stale relative to the round-01–03 + block
  A state of the wrapper. Goal: the repo should describe and stow exactly the final
  intended layout, with no dead code paths.
- Commit in `~/.dotfiles/codex-session` with a Conventional Commit covering blocks B + C
  (single commit; the cleanup is a direct corollary of the migration).

**In scope (Block D — cross-repo docs sync):**

- `~/DocsNNotes/`: locate every doc that mentions codex-session, codex profiles, the
  `settings/` vocabulary, the legacy `[profiles.*]` shape, or the nested
  `configs/profiles/` path. Common starting points: `tech/tools/claude-code/codex-*.md`,
  `tech/tools/codex/`, any "tools-i-use" or session-config notes. Update each to the
  final layout (sibling `profiles/`, `config-layers:`, codex v0.134+ contract). Commit in
  `~/DocsNNotes` with a Conventional Commit.
- `~/.dotfiles/claude/` and `~/.dotfiles/claude-session/`: scan for any file (commonly
  under `.claude/`, `.config/claude/`, or skill READMEs) that mentions codex-session or
  the legacy vocabulary. Update only those — leave the rest of the repos alone. If a repo
  has no codex-session-relevant references, no commit there. Otherwise one Conventional
  Commit per repo touched.

**Out of scope:**

- Re-stowing or running `stow` on host machines (manual user step, documented in the final
  report).
- Unrelated content in `~/DocsNNotes` and `~/.dotfiles/{claude,claude-session}` — only
  codex-session-relevant content is in scope. No drive-by edits.
- `.agents/skills/` bundle content in `~/.dotfiles/codex-session` — those are skill
  definitions, not codex-session config. Out of scope for the cleanup.

## Current State

### Block A (in `/workspaces/codex-session`)

- `src/config/mod.rs::ConfigRecipeConfig` has `configs_dir: Utf8PathBuf` (added in round
  02). No `profiles_dir` field yet — the profile dir is derived in
  `ConfigRecipePaths::profiles_dir()`.
- `src/services/config_recipe/composition.rs::ConfigRecipePaths` exposes
  `profiles_dir()` as a method returning `configs_dir.join("profiles")` (round 02).
- `src/services/config_recipe/mod.rs::compose()` calls `paths.profiles_dir()` when
  scanning the manifest-omits-list case (round 02).
- `src/commands/doctor.rs::check_legacy_profile_forms` (added in round 03) sweeps
  `configs_dir.join("profiles")` for legacy-syntax findings. It does NOT yet detect a
  nested `configs/profiles/` dir as a layout-migration finding.
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

### Block B (in `~/.dotfiles/codex-session`)

State as of plan generation (`find` output):

```
.gitignore
.config/codex-session/config.toml
.config/codex-session/config-recipes/default.yaml
.config/codex-session/configs/{projects,base,plugins}.toml      ← from an earlier ad-hoc partial migration
.config/codex-session/settings/{projects,base,plugins}.toml     ← legacy stow source, still on disk
.config/codex-session/settings.bak.20260528/{projects,base,plugins}.toml  ← stale backup
.agents/skills/*/SKILL.md                                       ← out of scope
```

`config-recipes/default.yaml` likely still uses `settings-layers:` (verify in step B1).
`config.toml` likely still has the old `settings/` comment trail (verify in step B1).

Both `settings/` and `configs/` exist on disk. They may have divergent content from the
ad-hoc partial migration. Block B authors the canonical `configs/` content and creates the
sibling `profiles/` dir; block C deletes `settings/` and `settings.bak.20260528/`.

### Block C (in `~/.dotfiles/codex-session`)

Cleanup candidates already identified:

- `.config/codex-session/settings/` (entire dir) — replaced by `configs/` + `profiles/`.
- `.config/codex-session/settings.bak.20260528/` (entire dir) — stale backup.
- Any reference to `settings-layers:` / `settings_dir` / `configs/profiles/` /
  `[profiles.*]` anywhere else in the repo (`.gitignore`, README, comments).

### Block D (cross-repo)

Pre-scan candidates (verified at step D1):

- `~/DocsNNotes/tech/tools/claude-code/codex-conventions.md` (touched in round 01).
- Any other `~/DocsNNotes/tech/tools/{claude-code,codex}/*.md` file naming codex-session
  vocabulary.
- `~/.dotfiles/claude/.config/claude/...` — any reference to codex-session config layout.
- `~/.dotfiles/claude-session/.config/claude-session/...` — same.

The exact files-to-touch list is built in step D1 via `grep -r` from the documented
starting points; the executor must NOT assume a fixed list — codex-session vocabulary may
appear in unexpected places.

### Existing Patterns

- Rust field rename / addition is compiler-driven: add the field, fix every constructor the
  compiler points at. Keep the `profiles_dir` field next to `configs_dir` in every struct
  for readability.
- Dotfiles repo uses Conventional Commits (`Feat(...)`, `Fix(...)`, `Chore(...)`,
  `Refactor(...)`). Block B + C commit prefix: `Refactor(config):`.
- `~/DocsNNotes` is also a git repo with Conventional Commits — verify with
  `git -C ~/DocsNNotes log --oneline -5` before authoring. Likely prefix:
  `Docs(codex):`.
- `~/.dotfiles/{claude,claude-session}` — same convention. Commit only if at least one
  file was changed.

## Implementation Steps

### Block A — Sibling `profiles/` restructure (in `/workspaces/codex-session`)

#### A1. Add `profiles_dir` to `ConfigRecipeConfig` and `FileConfigRecipeConfig`

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

Update the file→runtime config resolver (grep for `configs_dir:` to find the merge site)
to thread `profiles_dir` through the same way.

#### A2. Promote `profiles_dir` to a field in `ConfigRecipePaths`

In `src/services/config_recipe/composition.rs`:

```rust
pub(crate) struct ConfigRecipePaths {
    pub(crate) recipes_dir: Utf8PathBuf,
    pub(crate) configs_dir: Utf8PathBuf,
    pub(crate) profiles_dir: Utf8PathBuf,  // NEW first-class field
    pub(crate) cache_config: Option<Utf8PathBuf>,
}
```

Delete the `impl ConfigRecipePaths { fn profiles_dir(...) }` derived-method block from
round 02. The compiler will surface every call site.

#### A3. Update every `ConfigRecipePaths { … }` constructor

Grep for `ConfigRecipePaths {` across `src/`. Each constructor (in `mod.rs::compose`
callers, `gate.rs::extract_ping_config`, command modules) gains:

```rust
profiles_dir: ctx.config.config_recipe.profiles_dir.clone(),
```

next to the `configs_dir` line.

#### A4. Update `compose()` to use `paths.profiles_dir` directly

In `src/services/config_recipe/mod.rs::compose()`, replace `paths.profiles_dir()` (method
call) with `paths.profiles_dir.clone()` or `&paths.profiles_dir` (field access) at every
call site. The directory-scan and the manifest-declared-name path-join both use
`paths.profiles_dir` as the parent dir.

#### A5. Widen the doctor sweep to flag the obsolete nested layout

In `src/commands/doctor.rs::check_legacy_profile_forms`, after the existing legacy-form
checks, add:

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
`configs_dir.join("profiles")` — switch it to `&ctx.config.config_recipe.profiles_dir`.

#### A6. Update in-repo prose docs

`/workspaces/codex-session/README.md`:

- Line ~87 (filesystem-layout block): rewrite to show
  `~/.config/codex-session/configs/<layer>.toml` and a sibling
  `~/.config/codex-session/profiles/<name>.config.toml` rather than the nested form.
- Line ~110 (user-config prose explaining where overrides live): same edit — `profiles/`
  is a sibling of `configs/`, not a child.

`/workspaces/codex-session/docs/upstream-codex.md`:

- Line 147 (Mitigation in codex-session): rewrite `configs/profiles/ping.config.toml` →
  `profiles/ping.config.toml`. Adjust surrounding prose if it explicitly says "nested".
- Line 214 (§F6c Adjacent invariants): rewrite `configs/profiles/*.config.toml` →
  `profiles/*.config.toml`.
- Bump the `Last verified` date at the top of `docs/upstream-codex.md` to today.

`/workspaces/codex-session/CLAUDE.md`:

- § Codex config compatibility: the bullet about emitted output is correct as-is (the
  emitted shape `$CODEX_HOME/<name>.config.toml` siblings does not change). Add one
  parenthetical to the input-layer bullet clarifying that input layers live under
  `configs/` and per-profile overrides under sibling `profiles/`. Keep the edit minimal.

#### A7. Update `tests/support/mod.rs`

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

#### A8. Fix test fixtures that hardcoded the nested path

Grep `tests/` for `configs/profiles` and `configs_dir().join("profiles")` — there should
be zero matches after `write_profile_file` is updated, but inline string-literal fixtures
(e.g. doctor stderr assertions, config-status output assertions) may still hardcode the
nested path. Update each to the sibling form.

#### A9. Add doctor test for the new nested-layout finding

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

#### A10. Run gates and commit in `/workspaces/codex-session`

```bash
cd /workspaces/codex-session
just lint
just test
just precommit-all
```

Stage and commit:

```bash
git add src/ tests/ README.md CLAUDE.md docs/upstream-codex.md \
        .plan/01-todo/configs-rename-split-profiles/

git commit -m "$(cat <<'EOF'
Refactor(config): lift `profiles/` to sibling of `configs/` (round 04 block A)

Rounds 02-03 introduced the nested `configs/profiles/<name>.config.toml`
layout as an intermediate. This commit promotes `profiles_dir` to a
first-class field on `ConfigRecipeConfig` and `ConfigRecipePaths`
defaulting to `config_dir.join("profiles")`. The wrapper's input layout
is now `configs/<layer>.toml` + sibling `profiles/<name>.config.toml`.
Doctor sweep gains a finding for any surviving nested
`configs/profiles/` directory. README, docs/upstream-codex.md, and
CLAUDE.md are updated to describe the sibling layout. Tests pass.

References: docs/upstream-codex.md §F6b-§F6c, CLAUDE.md § Codex config
compatibility.
EOF
)"
```

### Block B — Dotfiles propagation (in `~/.dotfiles/codex-session`)

#### B1. Inspect starting state

```bash
cd /home/gu/.dotfiles/codex-session
git status --short
git log --oneline -5
ls -la .config/codex-session/
diff -r .config/codex-session/settings .config/codex-session/configs  # check divergence
cat .config/codex-session/configs/base.toml
cat .config/codex-session/configs/plugins.toml
cat .config/codex-session/configs/projects.toml
cat .config/codex-session/config-recipes/default.yaml
cat .config/codex-session/config.toml
```

Reconcile divergences: if `configs/` already exists and differs from `settings/`, treat
`settings/` as the authoritative source for content that block B needs (since `configs/`
came from an ad-hoc partial migration that block C will collapse). If `configs/` already
has the round-04 final shape (no `profile = "..."` / `[profiles.*]`), use it as-is.

#### B2. Author the final `configs/` content

Write (or rewrite) `.config/codex-session/configs/base.toml`:

```toml
# Portable codex defaults shared by every codex-session config-recipe.
# Per-profile overrides live in profiles/<name>.config.toml (sibling).

web_search = "live"

[features]
multi_agent = true
```

`.config/codex-session/configs/plugins.toml` (copy from current `settings/plugins.toml` —
no profile keys, no migration needed):

```toml
# Codex plugin enables + TUI new-user-experience state.

[plugins."google-calendar@openai-curated"]
enabled = true

[plugins."google-drive@openai-curated"]
enabled = true

[tui.model_availability_nux]
"gpt-5.5" = 4
```

`.config/codex-session/configs/projects.toml`: keep content identical to the current
`settings/projects.toml` (read it first; if it carries any `profile = "..."` or
`[profiles.*]`, extract them like base.toml — but it should not).

#### B3. Create the sibling `profiles/` directory and per-profile files

```bash
mkdir -p /home/gu/.dotfiles/codex-session/.config/codex-session/profiles
```

`.config/codex-session/profiles/deep.config.toml`:

```toml
# Deep reasoning profile — high effort, full sandbox.
# Selected with `codex --profile deep` (codex v0.134+ contract).

sandbox_mode = "danger-full-access"
model_reasoning_effort = "high"
plan_mode_reasoning_effort = "high"
```

`.config/codex-session/profiles/fast.config.toml`:

```toml
# Fast execution profile — gpt-5.4-mini, medium effort.
# Selected with `codex --profile fast`.
# Note: no gpt-5.5-mini exists yet; keep explicit 5.4-mini override.

sandbox_mode = "danger-full-access"
model = "gpt-5.4-mini"
model_reasoning_effort = "medium"
plan_mode_reasoning_effort = "medium"
```

`.config/codex-session/profiles/ping.config.toml`:

```toml
# Health-probe profile used by `codex-session account health`.
# Selected with `codex --profile ping`.

model = "gpt-5.4-mini"
model_reasoning_effort = "minimal"
```

#### B4. Update the manifest

Rewrite `.config/codex-session/config-recipes/default.yaml`:

```yaml
config-layers:
  - base
  - projects
  - plugins
```

(Profile overrides under sibling `profiles/` are emitted as siblings of `config.toml` at
compose time; `profile-files:` is omitted so all files under `profiles/` are emitted.)

#### B5. Update the wrapper-config comment header

Rewrite `.config/codex-session/config.toml`:

```toml
# codex-session wrapper config.
# Layer composition lives in config-recipes/ + configs/ (with per-profile
# overrides under sibling profiles/<name>.config.toml).

[config-recipe]
default = "default"
```

### Block C — Dotfiles cleanup (in `~/.dotfiles/codex-session`)

#### C1. Remove obsolete directories

```bash
cd /home/gu/.dotfiles/codex-session
git rm -r .config/codex-session/settings
git rm -r .config/codex-session/settings.bak.20260528
```

If `git rm -r` fails because the dir is untracked, fall back to `rm -rf` then `git
status`.

#### C2. Sweep for residual references

```bash
cd /home/gu/.dotfiles/codex-session
grep -rn "settings-layers\|settings_dir\|configs/profiles\|\[profiles\." \
    --exclude-dir=.git --exclude-dir=.agents 2>/dev/null
```

For each match outside `.agents/skills/`:

- If it's a comment / README / doc, update to the new vocabulary.
- If it's a config file (unexpected at this stage), fix it.
- If it's the `.gitignore`, leave it alone unless it mentions stale paths.

Read every non-skill file under `.config/codex-session/` end-to-end one more time and
confirm: no `settings/` references, no nested `configs/profiles/` references, no legacy
`[profiles.*]` blocks, no `settings-layers:` keys.

#### C3. Commit blocks B + C together

```bash
cd /home/gu/.dotfiles/codex-session
git status --short
git diff --stat

git add .config/codex-session/configs/ \
        .config/codex-session/profiles/ \
        .config/codex-session/config-recipes/default.yaml \
        .config/codex-session/config.toml

git commit -m "$(cat <<'EOF'
Refactor(config): adopt codex v0.134+ sibling profiles layout

Codex CLI v0.134.0 stopped accepting `profile = "..."` selectors and
`[profiles.<name>]` tables in `$CODEX_HOME/config.toml`. Per-profile
overrides now live in sibling files `<name>.config.toml` activated by
`--profile <name>`. codex-session's composer (rounds 02-03 of
`configs-rename-split-profiles`) emits the new shape, and round 04
finalized the input layout: `configs/<layer>.toml` for base layers and
sibling `profiles/<name>.config.toml` for per-profile overrides.

- Renamed `.config/codex-session/settings/` -> `configs/` (canonical
  base-layer content).
- Created sibling `.config/codex-session/profiles/` with
  `deep.config.toml`, `fast.config.toml`, `ping.config.toml` — bare
  top-level keys, no `[profiles.<name>]` header.
- Renamed manifest YAML field `settings-layers:` -> `config-layers:` in
  `config-recipes/default.yaml`.
- Updated comment headers in `config.toml`.
- Removed obsolete `.config/codex-session/settings.bak.20260528/`
  backup directory from an earlier ad-hoc migration.

References: docs/upstream-codex.md §F6b-§F6c in the codex-session
repo; <https://developers.openai.com/codex/config-advanced#profiles>.
EOF
)"
```

### Block D — Cross-repo docs sync

#### D1. Discover codex-session-relevant docs in each external repo

```bash
# DocsNNotes
grep -rn "codex-session\|settings-layers\|configs/profiles\|\[profiles\." \
    ~/DocsNNotes 2>/dev/null \
    | grep -v "/\.git/" \
    | tee /tmp/round04-docsnotes-hits.txt

# Claude dotfiles
grep -rn "codex-session\|settings-layers\|configs/profiles" \
    ~/.dotfiles/claude 2>/dev/null \
    | grep -v "/\.git/" \
    | tee /tmp/round04-claude-hits.txt

# Claude-session dotfiles
grep -rn "codex-session\|settings-layers\|configs/profiles" \
    ~/.dotfiles/claude-session 2>/dev/null \
    | grep -v "/\.git/" \
    | tee /tmp/round04-claude-session-hits.txt
```

Triage each hit: codex-session-relevant → edit; unrelated → skip. Record the edit list.

#### D2. Update `~/DocsNNotes/` codex-session-relevant docs

For each in-scope file (likely starting points: `tech/tools/claude-code/codex-conventions.md`
— touched in round 01 — and any other doc in `tech/tools/{claude-code,codex}/`):

- Rewrite filesystem-layout sections to use sibling `profiles/<name>.config.toml`.
- Rewrite any `settings/` vocabulary to `configs/`.
- Rewrite `settings-layers:` to `config-layers:`.
- Add or update the codex v0.134+ contract reference if missing.
- Keep edits minimal and surgical — do NOT rewrite unrelated sections.

Commit in `~/DocsNNotes`:

```bash
cd ~/DocsNNotes
git status --short
git add <list of files>
git commit -m "$(cat <<'EOF'
Docs(codex): sync codex-session notes to sibling profiles layout

codex-session migrated to the codex v0.134+ split-profile contract and
adopted a sibling `profiles/` directory layout (round 04 of the
configs-rename-split-profiles plan in /workspaces/codex-session). This
commit updates the codex-session-relevant notes to match: filesystem
layout (`configs/<layer>.toml` + sibling `profiles/<name>.config.toml`),
manifest YAML field `config-layers:`, and the codex v0.134+ profile
contract reference.
EOF
)"
```

#### D3. Update `~/.dotfiles/claude/` codex-session-relevant content

If `/tmp/round04-claude-hits.txt` is empty, skip — no commit. Otherwise edit only the
in-scope files (codex-session-relevant) and commit:

```bash
cd ~/.dotfiles/claude
git add <list of files>
git commit -m "$(cat <<'EOF'
Docs(codex): sync codex-session references to sibling profiles layout

codex-session adopted a sibling `profiles/` directory layout for
per-profile overrides (round 04 of configs-rename-split-profiles).
Updates only the codex-session-relevant references in this repo; all
other content is untouched.
EOF
)"
```

#### D4. Update `~/.dotfiles/claude-session/` codex-session-relevant content

If `/tmp/round04-claude-session-hits.txt` is empty, skip — no commit. Otherwise edit only
the in-scope files and commit with the same template as D3, swapping the repo name in the
commit message.

### Final verification — manual host re-stow + smoke test

This is the user's responsibility, not the executor's. Tell the user in the implementation
report:

```bash
# On each affected host, after pulling the dotfiles repo:
cd ~/.dotfiles
rm -rf ~/.config/codex-session/settings  # legacy stowed tree
rm -rf ~/.config/codex-session/configs/profiles  # obsolete nested dir if present
stow codex-session
# Verify the sibling layout is in place:
ls ~/.config/codex-session/{configs,profiles}/
# Smoke test:
codex-session doctor
codex-session account health
codex-session --account auto exec --profile deep "say ok"
```

If the executor is running on a host where re-stowing has already happened, run the smoke
test directly and include the result in the implementation report. If the executor cannot
re-stow (devcontainer / sandboxed environment), document that fact and rely on the
in-repo `just test` gates from block A.

### Final Step: Update plan index

Update `.plan/01-todo/configs-rename-split-profiles/README.md` (in `/workspaces/codex-session`):

1. In the `## Execution Order` table, find the row for round 04.
2. Change `Status` from `todo` to `done`.
3. Change `Completed` from `--` to today's date (`YYYY-MM-DD`).
4. In the README.md header blockquote, change `Status: todo` to `Status: done`.
5. Move the plan directory to done:

```bash
cd /workspaces/codex-session
mkdir -p .plan/02-done && mv .plan/01-todo/configs-rename-split-profiles .plan/02-done/configs-rename-split-profiles
```

(Include this directory move in a follow-up commit in `/workspaces/codex-session`, or
amend the block-A commit if the executor prefers a single commit.)

## Acceptance Criteria

**Block A (in `/workspaces/codex-session`):**

- [ ] `ConfigRecipeConfig.profiles_dir: Utf8PathBuf` exists; default constructor sets it
      to `config_dir.join("profiles")`.
- [ ] `FileConfigRecipeConfig.profiles_dir: Option<Utf8PathBuf>` exists with
      `#[serde(rename = "profiles-dir", default)]` and is merged into the runtime config
      next to `configs_dir`.
- [ ] `ConfigRecipePaths.profiles_dir: Utf8PathBuf` is a first-class field; the derived
      `profiles_dir()` method from round 02 is deleted.
- [ ] Every `ConfigRecipePaths { … }` constructor in `src/` passes `profiles_dir`.
- [ ] `compose()` in `src/services/config_recipe/mod.rs` scans
      `paths.profiles_dir` (field, not `configs_dir.join("profiles")`).
- [ ] `doctor::check_legacy_profile_forms` emits a warning when an obsolete nested
      `<configs_dir>/profiles/` dir exists and differs from `<profiles_dir>`. Warning
      cites `docs/upstream-codex.md §F6c` and names the sibling target path.
- [ ] `README.md` filesystem-layout and prose sections describe sibling
      `profiles/<name>.config.toml`. No `configs/profiles/` substring survives in
      `README.md`.
- [ ] `docs/upstream-codex.md` lines previously naming `configs/profiles/...` now name
      sibling `profiles/...`. `Last verified` date bumped to today.
- [ ] `CLAUDE.md` § Codex config compatibility input-layer bullet clarifies sibling
      layout.
- [ ] `tests/support/mod.rs` exposes `profiles_dir()` and `write_profile_file(name,
      body)` rooted at `<config_home>/codex-session/profiles/`.
- [ ] `grep -rn "configs/profiles" src/ tests/ README.md CLAUDE.md docs/` returns no
      matches.
- [ ] New doctor test `doctor_warns_on_nested_configs_profiles_dir` exists and passes.
- [ ] `just lint`, `just test`, and `just precommit-all` all pass.
- [ ] One commit in `/workspaces/codex-session` captures the restructure with a
      Conventional Commit message citing round 04 block A.

**Block B + C (in `~/.dotfiles/codex-session`):**

- [ ] `.config/codex-session/settings/` directory no longer exists.
- [ ] `.config/codex-session/settings.bak.20260528/` directory no longer exists.
- [ ] `.config/codex-session/configs/` contains `base.toml`, `plugins.toml`,
      `projects.toml` — content matches the round-04 specs (no `profile = "..."` /
      `[profiles.*]`).
- [ ] `.config/codex-session/profiles/` (sibling of `configs/`) contains
      `deep.config.toml`, `fast.config.toml`, `ping.config.toml` with bare top-level
      keys.
- [ ] `config-recipes/default.yaml` uses `config-layers:` and lists `base`, `projects`,
      `plugins` in that order.
- [ ] `config.toml` comment header references `configs/` and sibling `profiles/`.
- [ ] `grep -rn "settings-layers\|settings_dir\|configs/profiles\|\[profiles\."
      --exclude-dir=.git --exclude-dir=.agents` returns no matches.
- [ ] A single commit in `~/.dotfiles/codex-session` captures blocks B + C with a
      Conventional Commit message citing codex v0.134+ and the sibling profiles layout.

**Block D (cross-repo):**

- [ ] `~/DocsNNotes` has at least one updated codex-session-relevant doc (or, if the
      pre-scan found none stale, the implementation report documents that no updates
      were needed). Where updates happen, one Conventional Commit captures them.
- [ ] `~/.dotfiles/claude` and `~/.dotfiles/claude-session` are inspected; if updates
      are needed, each gets one Conventional Commit. If not, the report documents the
      no-op.
- [ ] No drive-by edits outside codex-session-relevant content.

**Plan index:**

- [ ] Plan `README.md` execution order table shows round 04 as `done` with today's date.
- [ ] Plan `README.md` header status is `done`.
- [ ] Plan directory moved from `.plan/01-todo/configs-rename-split-profiles` to
      `.plan/02-done/configs-rename-split-profiles`.

## Next Round

This is the final round.

After this round, the wrapper crate, its tests, all in-repo documentation, the dotfiles
source-of-truth, and the cross-repo docs in `~/DocsNNotes` and
`~/.dotfiles/{claude,claude-session}` are all aligned with codex v0.134+'s split-profile
input contract AND with the sibling `profiles/` input layout. The composability contract
codified in round 01 (CLAUDE.md § Codex config compatibility) becomes the standing rule
for future upstream config changes.
