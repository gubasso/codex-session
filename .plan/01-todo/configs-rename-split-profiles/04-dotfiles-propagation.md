# Round 04: Dotfiles Propagation (`~/.dotfiles/codex-session`)

> Plan: configs-rename-split-profiles | Round: 04 of 04 | Complexity: L
> Generated: 2026-05-28 | Repo: /workspaces/codex-session

## Context

The user maintains a stow-managed source-of-truth for the codex-session user config at
`~/.dotfiles/codex-session/`. The tree currently lives at
`~/.dotfiles/codex-session/.config/codex-session/{config.toml, config-recipes/default.yaml,
settings/{base.toml, plugins.toml, projects.toml}}` and gets stowed onto each host as
`~/.config/codex-session/...`.

After rounds 01–03, the wrapper crate expects the new layout:

- `configs/` instead of `settings/`,
- per-profile files at `configs/profiles/<name>.config.toml` (bare top-level keys, no
  `[profiles.<name>]` header) for codex v0.134+ compatibility,
- the manifest YAML field `config-layers:` instead of `settings-layers:`,
- a new optional `profile-files:` field (omit it to emit every file under `configs/profiles/`).

This round migrates the dotfiles repo to match. After the commit lands and the user re-stows
on each host, codex-session works again end-to-end (heartbeat probe + planning + execution).

The dotfiles repo is a **separate git repository** rooted at `/home/gu/.dotfiles/codex-session`.
All file changes and the commit in this round happen there — NOT in
`/workspaces/codex-session`.

## Previous Rounds

**Round 01 (expected state):** docs codify the API-compat principle. README.md filesystem
layout, `CLAUDE.md` § Codex config compatibility, `docs/upstream-codex.md` §F6b–§F6c, and
`~/DocsNNotes/.../codex-conventions.md` all describe the v0.134+ split-file layout.

**Round 02 (expected state):** composer emits `configs/<name>.config.toml` siblings 1:1;
manifest field is `config-layers:` (required) + `profile-files:` (optional); composer rejects
legacy `profile = "..."` and `[profiles.*]` at input layer + emitted output. `configs_dir`
replaces `settings_dir` throughout the crate.

**Round 03 (expected state):** heartbeat probe reads `configs/profiles/ping.config.toml`
directly; cache file renamed to `configs.toml`; doctor surfaces a one-line migration hint
when legacy shapes are detected.

## Scope of This Round

**In scope (all paths under `/home/gu/.dotfiles/codex-session/`):**

- Rename the directory `.config/codex-session/settings/` → `.config/codex-session/configs/`.
- Strip `profile = "deep"` from `configs/base.toml`. Strip the
  `[profiles.{deep,fast,ping}]` tables from `configs/base.toml`. Keep everything else
  (`web_search`, `[features]`).
- Create `configs/profiles/deep.config.toml`, `configs/profiles/fast.config.toml`, and
  `configs/profiles/ping.config.toml` with the bare keys that previously lived under
  `[profiles.{deep,fast,ping}]` (no headers).
- Update `.config/codex-session/config-recipes/default.yaml`: rename
  `settings-layers:` → `config-layers:`. Do NOT add `profile-files:` — the omit-and-emit-all
  default matches the user's prior intent (all three profiles available, none auto-activated).
- Verify `.config/codex-session/config.toml` (wrapper config) does not need to change. (After
  round 02, the wrapper still uses the same `[config-recipe]` section and `default = ...`
  field — only the underlying dir name changed.)
- Commit in the dotfiles repo with a Conventional Commit message describing the migration.

**Out of scope:**

- Any change to `/workspaces/codex-session/` (rounds 01–03 already covered).
- Re-stowing or running `stow` on host machines. This is a manual user step performed AFTER
  the commit lands.

## Current State

### Key Files (in `~/.dotfiles/codex-session/`)

- `.gitignore` (already present).

- `.config/codex-session/config.toml`:

  ```toml
  # codex-session wrapper config.
  # Layer composition lives in config-recipes/ + settings/.

  [config-recipe]
  default = "default"
  ```

  Update the comment to say "config-recipes/ + configs/" (one-line cosmetic change so the
  trail of crumbs matches reality).

- `.config/codex-session/config-recipes/default.yaml`:

  ```yaml
  settings-layers:
    - base
    - projects
    - plugins
  ```

  Rename top-level key to `config-layers:`. Layer order and names are unchanged.

- `.config/codex-session/settings/base.toml`:

  ```toml
  # Portable Codex defaults shared by every codex-session config-recipe.

  profile = "deep"
  web_search = "live"

  [profiles.deep]
  sandbox_mode = "danger-full-access"
  model_reasoning_effort = "high"
  plan_mode_reasoning_effort = "high"

  [profiles.fast]
  # No gpt-5.5-mini exists yet; keep explicit 5.4-mini override.
  sandbox_mode = "danger-full-access"
  model = "gpt-5.4-mini"
  model_reasoning_effort = "medium"
  plan_mode_reasoning_effort = "medium"

  [profiles.ping]
  model = "gpt-5.4-mini"
  model_reasoning_effort = "minimal"

  [features]
  multi_agent = true
  ```

  This file gets stripped of `profile = "deep"` and all three `[profiles.*]` tables. The
  remaining keys (`web_search`, `[features]`) live in the renamed
  `.config/codex-session/configs/base.toml`.

- `.config/codex-session/settings/plugins.toml`:

  ```toml
  # Codex plugin enables + TUI new-user-experience state.

  [plugins."google-calendar@openai-curated"]
  enabled = true

  [plugins."google-drive@openai-curated"]
  enabled = true

  [tui.model_availability_nux]
  "gpt-5.5" = 4
  ```

  No content change — just moves to `.config/codex-session/configs/plugins.toml` via the
  directory rename.

- `.config/codex-session/settings/projects.toml` — content unknown; treat as opaque. Moves to
  `.config/codex-session/configs/projects.toml` via the directory rename. Read it during step
  3 to confirm it does NOT contain `profile = "..."` or `[profiles.*]` — if it does (it
  shouldn't), extract those too.

### Existing Patterns

- The dotfiles repo uses Conventional Commits (`Feat(...)`, `Fix(...)`, `Chore(...)`). Use the
  prefix that fits — likely `Chore(config)` or `Refactor(config)` for a structural migration.
- Dotfiles tree is stow-managed: each top-level subdirectory (`.config/`, `.agents/`) is
  the stow source root. Preserve dotfile semantics (no spurious top-level files).
- The dotfiles repo also ships skill bundles under `.agents/skills/`. Those are out of scope
  for this round.

## Implementation Steps

### Step 1: Inspect the dotfiles tree and confirm starting state

```bash
cd /home/gu/.dotfiles/codex-session
git status --short
git log --oneline -5
ls -la .config/codex-session/
ls -la .config/codex-session/settings/
cat .config/codex-session/settings/base.toml
cat .config/codex-session/settings/plugins.toml
cat .config/codex-session/settings/projects.toml
cat .config/codex-session/config-recipes/default.yaml
cat .config/codex-session/config.toml
```

Note the actual content of `projects.toml`. If it contains `profile = "..."` or
`[profiles.*]`, you will need to extract those into profile files too (unlikely; this file
typically holds `[projects.*]` trust entries written by the cache layer, not by user
config).

### Step 2: Rename the directory

```bash
cd /home/gu/.dotfiles/codex-session/.config/codex-session
git mv settings configs
git status --short
```

Use `git mv` so history follows the directory. Verify all three files moved
(`base.toml`, `plugins.toml`, `projects.toml`).

### Step 3: Strip legacy profile shape from `configs/base.toml`

Rewrite `.config/codex-session/configs/base.toml` to:

```toml
# Portable codex defaults shared by every codex-session config-recipe.
# Per-profile overrides live in configs/profiles/<name>.config.toml.

web_search = "live"

[features]
multi_agent = true
```

### Step 4: Create the three per-profile files

Create the directory:

```bash
mkdir -p /home/gu/.dotfiles/codex-session/.config/codex-session/configs/profiles
```

Create `.config/codex-session/configs/profiles/deep.config.toml`:

```toml
# Deep reasoning profile — high effort, full sandbox.
# Selected with `codex --profile deep` (codex v0.134+ contract).

sandbox_mode = "danger-full-access"
model_reasoning_effort = "high"
plan_mode_reasoning_effort = "high"
```

Create `.config/codex-session/configs/profiles/fast.config.toml`:

```toml
# Fast execution profile — gpt-5.4-mini, medium effort.
# Selected with `codex --profile fast`.
# Note: no gpt-5.5-mini exists yet; keep explicit 5.4-mini override.

sandbox_mode = "danger-full-access"
model = "gpt-5.4-mini"
model_reasoning_effort = "medium"
plan_mode_reasoning_effort = "medium"
```

Create `.config/codex-session/configs/profiles/ping.config.toml`:

```toml
# Health-probe profile used by `codex-session account health`.
# Selected with `codex --profile ping`.

model = "gpt-5.4-mini"
model_reasoning_effort = "minimal"
```

### Step 5: Update the manifest

Rewrite `.config/codex-session/config-recipes/default.yaml` to:

```yaml
config-layers:
  - base
  - projects
  - plugins
```

(Optional: add a `# Profile overrides under configs/profiles/ are emitted as siblings of
config.toml at compose time.` comment line if dotfiles style permits.)

### Step 6: Update wrapper-config comment trail

Rewrite `.config/codex-session/config.toml` comment header:

```toml
# codex-session wrapper config.
# Layer composition lives in config-recipes/ + configs/ (with per-profile
# overrides under configs/profiles/<name>.config.toml).

[config-recipe]
default = "default"
```

### Step 7: Stage and commit in the dotfiles repo

```bash
cd /home/gu/.dotfiles/codex-session
git status --short
git diff --stat
```

Stage and commit:

```bash
git add .config/codex-session/configs/ \
        .config/codex-session/config-recipes/default.yaml \
        .config/codex-session/config.toml

git commit -m "$(cat <<'EOF'
Refactor(config): adopt codex v0.134+ split-profile layout

Codex CLI v0.134.0 stopped accepting `profile = "..."` selectors and
`[profiles.<name>]` tables in `$CODEX_HOME/config.toml`. Per-profile
overrides now live in sibling files `<name>.config.toml` with bare
top-level keys, activated by `--profile <name>`. codex-session's
composer (rounds 02-03 of `configs-rename-split-profiles`) emits the
new shape; this commit migrates the user-config source-of-truth to
match.

- Renamed `.config/codex-session/settings/` -> `configs/`.
- Stripped `profile = "deep"` and `[profiles.{deep,fast,ping}]` from
  `configs/base.toml`.
- Extracted each profile into `configs/profiles/<name>.config.toml`
  with bare top-level keys (no `[profiles.<name>]` header).
- Renamed manifest YAML field `settings-layers:` -> `config-layers:`
  in `config-recipes/default.yaml`.
- Updated comment headers in `config.toml`.

References: docs/upstream-codex.md §F6b-§F6c in the codex-session
repo; <https://developers.openai.com/codex/config-advanced#profiles>.
EOF
)"
```

### Step 8: Manual host re-stow (DO NOT automate)

Tell the user (in the final report) that they must re-stow the dotfiles on every host that
relies on `~/.config/codex-session/`. Suggested command (user runs it manually, not the
executor):

```bash
# On each affected host, after pulling the dotfiles repo:
cd ~/.dotfiles
rm -rf ~/.config/codex-session/settings  # old stowed tree
stow codex-session
# Verify:
ls ~/.config/codex-session/configs/profiles/
```

Document the manual step but **do not** run `stow` from this round. Stow operates on
real symlinks and reflects across the user's full host config — the executor must not modify
that state.

### Step 9: Verify end-to-end on the current host (smoke test)

If the executor is running on a host where re-stowing succeeded (or where
`~/.config/codex-session/configs/` already exists and matches the new layout), run:

```bash
codex-session doctor
codex-session config status
codex-session account health
codex-session --account auto exec --profile deep "say ok"
codex-session --account auto exec --profile fast "say ok"
```

Each command should succeed without the v0.134+ legacy-form rejection. Inspect a fresh
session dir:

```bash
ls ~/.local/state/codex-session/sessions/ | tail -1 | xargs -I{} ls ~/.local/state/codex-session/sessions/{}
```

Confirm the emitted tree contains `config.toml` (no `profile`/`[profiles.*]`) plus sibling
`deep.config.toml`, `fast.config.toml`, `ping.config.toml`.

If the host does not yet have a re-stowed config (e.g. executor runs in a devcontainer with
no `~/.config/codex-session/`), skip the smoke test and document that fact in the
implementation summary.

### Final Step: Update plan index

Update `.plan/01-todo/configs-rename-split-profiles/README.md` (in the
`/workspaces/codex-session` repo, NOT the dotfiles repo):

1. In the `## Execution Order` table, find the row for round 04.
2. Change `Status` from `todo` to `done`.
3. Change `Completed` from `--` to today's date (`YYYY-MM-DD`).
4. In the README.md header blockquote, change `Status: todo` to `Status: done`.
5. Move the plan directory to done:

```bash
cd /workspaces/codex-session
mkdir -p .plan/02-done && mv .plan/01-todo/configs-rename-split-profiles .plan/02-done/configs-rename-split-profiles
```

## Acceptance Criteria

- [ ] `~/.dotfiles/codex-session/.config/codex-session/settings/` directory no longer exists.
- [ ] `~/.dotfiles/codex-session/.config/codex-session/configs/` exists with
      `base.toml`, `plugins.toml`, `projects.toml` (renamed via `git mv` so history follows).
- [ ] `configs/base.toml` contains no `profile = "..."` and no `[profiles.*]`. It retains
      `web_search` and `[features]`.
- [ ] `configs/profiles/deep.config.toml`, `configs/profiles/fast.config.toml`, and
      `configs/profiles/ping.config.toml` exist with bare top-level keys (no
      `[profiles.<name>]` header) and content matching the previous `[profiles.*]` blocks.
- [ ] `config-recipes/default.yaml` uses `config-layers:` (not `settings-layers:`). Layer
      list unchanged: `base`, `projects`, `plugins`.
- [ ] `.config/codex-session/config.toml` comment header references `configs/` and
      `configs/profiles/`.
- [ ] A single commit in `~/.dotfiles/codex-session` captures all of the above with a
      Conventional Commit message that cites codex v0.134+ as the driver.
- [ ] If the host supports it: `codex-session doctor`, `codex-session account health`, and
      `codex-session --account auto exec --profile deep "say ok"` all succeed without the
      v0.134+ legacy-form rejection. If the host does not support a smoke test, the
      implementation report documents that fact.
- [ ] Plan `README.md` execution order table shows round 04 as `done` with today's date.
- [ ] Plan `README.md` header status is `done`.
- [ ] Plan directory moved from `.plan/01-todo/configs-rename-split-profiles` to
      `.plan/02-done/configs-rename-split-profiles`.

## Next Round

This is the final round.

After this round, the wrapper crate, its tests, all documentation (in-repo and DocsNNotes),
and the dotfiles source-of-truth are all aligned with codex v0.134+'s split-profile input
contract. The composability contract codified in round 01 (CLAUDE.md § Codex config
compatibility) becomes the standing rule for future upstream config changes.
