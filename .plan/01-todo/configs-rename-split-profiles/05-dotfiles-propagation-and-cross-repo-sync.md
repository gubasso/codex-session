# Round 05: Dotfiles Propagation + Cleanup + Cross-Repo Docs Sync

> Plan: configs-rename-split-profiles | Round: 05 of 05 | Complexity: L
> Generated: 2026-05-28 | Repos touched: `~/.dotfiles/codex-session`,
> `~/DocsNNotes`, `~/.dotfiles/claude`, `~/.dotfiles/claude-session`

## Context

This round finishes the configs-rename-split-profiles plan in one prex
session, in three ordered blocks. Round 04 already landed the wrapper-side
changes (codex v0.134+ fail-fast gate + sibling `profiles/` restructure);
this round propagates the same layout to the dotfiles source-of-truth, prunes
obsolete leftovers, and syncs codex-session-relevant docs across the
external repos.

1. **Block A — Dotfiles propagation** (in `~/.dotfiles/codex-session`):
  migrate the stow-managed source-of-truth to the final sibling layout.
  Rename `settings/` → `configs/`, extract `[profiles.{deep,fast,ping}]`
  blocks into the new sibling `profiles/<name>.config.toml` files, update
  `default.yaml`'s manifest key `settings-layers:` → `config-layers:`.
2. **Block B — Dotfiles cleanup** (in `~/.dotfiles/codex-session`): remove
  obsolete leftovers from prior plan iterations (an existing
  `settings.bak.20260528/` backup and a half-migrated `configs/` tree
  alongside the legacy `settings/`) and prune anything no longer relevant
  under the new sibling layout. Keep the repo lean and on-par with the
  final wrapper implementation.
3. **Block C — Cross-repo docs sync**: sweep `~/DocsNNotes` and
  `~/.dotfiles/{claude,claude-session}` for codex-session-relevant docs
  that still describe pre-v0.134 / nested-layout / `settings/`-vocabulary
  state and update each to match. Scope is strictly **codex-session-relevant
  content only** — unrelated Claude/Claude-session config files and skill
  bundles are not touched.

Each affected repo gets its own commit. The four repos are independent git
repos; the executor must `cd` into each one explicitly and not cross commit
boundaries.

The principle codified in round 01 (`CLAUDE.md` § Codex config compatibility)
drives every edit in every block.

## Previous Rounds

**Round 01 (expected state):** docs codify the API-compat principle.
`README.md` filesystem layout, `CLAUDE.md` § Codex config compatibility,
`docs/upstream-codex.md` §F6b–§F6c, and
`~/DocsNNotes/.../codex-conventions.md` describe the v0.134+ contract.
Round 04 already rewrote the in-repo doc paragraphs to the sibling layout;
this round mirrors that update across external repos.

**Round 02 (expected state):** composer emits `<name>.config.toml` siblings
under `$CODEX_HOME/`. Manifest field is `config-layers:` + optional
`profile-files:`.

**Round 03 (expected state):** heartbeat probe reads the active recipe's ping
profile file via `composition.profile_files`; cache file renamed to
`configs.toml`; doctor surfaces a one-line migration hint when legacy shapes
are detected.

**Round 04 (expected state):** wrapper crate enforces codex >= 0.134.0 at
every child-invoking code path and mirrors the gate as a doctor check
(`check_codex_version_minimum`). `profiles_dir` is a first-class field on
`ConfigRecipeConfig` / `FileConfigRecipeConfig` / `ConfigRecipePaths`
defaulting to `config_dir.join("profiles")` (sibling of `configs_dir`).
Doctor sweep flags any surviving nested `configs/profiles/` layout.
`README.md`, `docs/upstream-codex.md`, and `CLAUDE.md` describe the sibling
layout. The wrapper-side commit is in place; this round is the
external-repo follow-up.

## Scope of This Round

### Block A — Dotfiles propagation (in `~/.dotfiles/codex-session`)

- Rename `.config/codex-session/settings/` → `.config/codex-session/configs/`
  (via `git mv`) — but note that `.config/codex-session/configs/` already
  exists on disk from an earlier ad-hoc migration. Block B handles the
  reconciliation between the two; block A authors the *final* target state
  for `configs/` (base config layers, no profile keys).
- Author the final `configs/base.toml` content: strip `profile = "deep"`
  and the `[profiles.{deep,fast,ping}]` tables. Keep `web_search` and
  `[features]`.
- Author the final `configs/plugins.toml` and `configs/projects.toml`
  content: identical to the current `settings/` versions (no profile keys
  to extract).
- Create `.config/codex-session/profiles/` (sibling of `configs/`) with
  `deep.config.toml`, `fast.config.toml`, `ping.config.toml` — bare
  top-level keys, no `[profiles.<name>]` header.
- Update `.config/codex-session/config-recipes/default.yaml`: rename
  `settings-layers:` → `config-layers:`. Do not add `profile-files:`
  (omit-and-emit-all matches the user's prior intent: all three profiles
  available, none auto-activated).
- Update the comment trail in `.config/codex-session/config.toml` to
  reference `configs/` and `profiles/` (sibling) — drop the old `settings/`
  mention.

### Block B — Dotfiles cleanup (in `~/.dotfiles/codex-session`)

- Delete `.config/codex-session/settings.bak.20260528/` (entire dir) —
  leftover from a prior ad-hoc migration; obsolete under the new layout.
- Delete `.config/codex-session/settings/` (the legacy stow source) —
  replaced by `configs/` (base layers) + `profiles/` (overrides).
- Review every other file under `~/.dotfiles/codex-session/.config/codex-session/`
  and the repo root (excluding `.agents/skills/` — that's an unrelated
  bundle): remove anything that references the old `settings/` vocabulary,
  the legacy `[profiles.*]` shape, the nested `configs/profiles/` path, or
  is otherwise stale relative to the round-01–04 state of the wrapper.
  Goal: the repo should describe and stow exactly the final intended
  layout, with no dead code paths.
- Commit in `~/.dotfiles/codex-session` with a Conventional Commit covering
  blocks A + B (single commit; the cleanup is a direct corollary of the
  migration).

### Block C — Cross-repo docs sync

- `~/DocsNNotes/`: locate every doc that mentions codex-session, codex
  profiles, the `settings/` vocabulary, the legacy `[profiles.*]` shape,
  or the nested `configs/profiles/` path. Common starting points:
  `tech/tools/claude-code/codex-*.md`, `tech/tools/codex/`, any
  "tools-i-use" or session-config notes. Update each to the final layout
  (sibling `profiles/`, `config-layers:`, codex v0.134+ contract). Commit
  in `~/DocsNNotes` with a Conventional Commit.
- `~/.dotfiles/claude/` and `~/.dotfiles/claude-session/`: scan for any
  file (commonly under `.claude/`, `.config/claude/`, or skill READMEs)
  that mentions codex-session or the legacy vocabulary. Update only those
  — leave the rest of the repos alone. If a repo has no
  codex-session-relevant references, no commit there. Otherwise one
  Conventional Commit per repo touched.

### Out of scope

- Re-stowing or running `stow` on host machines (manual user step,
  documented in the final report).
- Unrelated content in `~/DocsNNotes` and
  `~/.dotfiles/{claude,claude-session}` — only codex-session-relevant
  content is in scope. No drive-by edits.
- `.agents/skills/` bundle content in `~/.dotfiles/codex-session` — those
  are skill definitions, not codex-session config. Out of scope for the
  cleanup.
- Any further code edits in `/workspaces/codex-session`. Round 04 finalized
  the wrapper crate; this round only touches external repos.

## Current State

### Block A (in `~/.dotfiles/codex-session`)

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

`config-recipes/default.yaml` likely still uses `settings-layers:` (verify
in step A1). `config.toml` likely still has the old `settings/` comment
trail (verify in step A1).

Both `settings/` and `configs/` exist on disk. They may have divergent
content from the ad-hoc partial migration. Block A authors the canonical
`configs/` content and creates the sibling `profiles/` dir; block B
deletes `settings/` and `settings.bak.20260528/`.

### Block B (in `~/.dotfiles/codex-session`)

Cleanup candidates already identified:

- `.config/codex-session/settings/` (entire dir) — replaced by `configs/`
  + `profiles/`.
- `.config/codex-session/settings.bak.20260528/` (entire dir) — stale
  backup.
- Any reference to `settings-layers:` / `settings_dir` / `configs/profiles/`
  / `[profiles.*]` anywhere else in the repo (`.gitignore`, README,
  comments).

### Block C (cross-repo)

Pre-scan candidates (verified at step C1):

- `~/DocsNNotes/tech/tools/claude-code/codex-conventions.md` (touched in
  round 01).
- Any other `~/DocsNNotes/tech/tools/{claude-code,codex}/*.md` file naming
  codex-session vocabulary.
- `~/.dotfiles/claude/.config/claude/...` — any reference to codex-session
  config layout.
- `~/.dotfiles/claude-session/.config/claude-session/...` — same.

The exact files-to-touch list is built in step C1 via `grep -r` from the
documented starting points; the executor must NOT assume a fixed list —
codex-session vocabulary may appear in unexpected places.

### Existing Patterns

- Dotfiles repo uses Conventional Commits (`Feat(...)`, `Fix(...)`,
  `Chore(...)`, `Refactor(...)`). Block A + B commit prefix:
  `Refactor(config):`.
- `~/DocsNNotes` is also a git repo with Conventional Commits — verify
  with `git -C ~/DocsNNotes log --oneline -5` before authoring. Likely
  prefix: `Docs(codex):`.
- `~/.dotfiles/{claude,claude-session}` — same convention. Commit only if
  at least one file was changed.

## Implementation Steps

### Block A — Dotfiles propagation (in `~/.dotfiles/codex-session`)

#### A1. Inspect starting state

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

Reconcile divergences: if `configs/` already exists and differs from
`settings/`, treat `settings/` as the authoritative source for content
that block A needs (since `configs/` came from an ad-hoc partial migration
that block B will collapse). If `configs/` already has the round-05 final
shape (no `profile = "..."` / `[profiles.*]`), use it as-is.

#### A2. Author the final `configs/` content

Write (or rewrite) `.config/codex-session/configs/base.toml`:

```toml
# Portable codex defaults shared by every codex-session config-recipe.
# Per-profile overrides live in profiles/<name>.config.toml (sibling).

web_search = "live"

[features]
multi_agent = true
```

`.config/codex-session/configs/plugins.toml` (copy from current
`settings/plugins.toml` — no profile keys, no migration needed):

```toml
# Codex plugin enables + TUI new-user-experience state.

[plugins."google-calendar@openai-curated"]
enabled = true

[plugins."google-drive@openai-curated"]
enabled = true

[tui.model_availability_nux]
"gpt-5.5" = 4
```

`.config/codex-session/configs/projects.toml`: keep content identical to
the current `settings/projects.toml` (read it first; if it carries any
`profile = "..."` or `[profiles.*]`, extract them like base.toml — but it
should not).

#### A3. Create the sibling `profiles/` directory and per-profile files

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

#### A4. Update the manifest

Rewrite `.config/codex-session/config-recipes/default.yaml`:

```yaml
config-layers:
  - base
  - projects
  - plugins
```

(Profile overrides under sibling `profiles/` are emitted as siblings of
`config.toml` at compose time; `profile-files:` is omitted so all files
under `profiles/` are emitted.)

#### A5. Update the wrapper-config comment header

Rewrite `.config/codex-session/config.toml`:

```toml
# codex-session wrapper config.
# Layer composition lives in config-recipes/ + configs/ (with per-profile
# overrides under sibling profiles/<name>.config.toml).

[config-recipe]
default = "default"
```

### Block B — Dotfiles cleanup (in `~/.dotfiles/codex-session`)

#### B1. Remove obsolete directories

```bash
cd /home/gu/.dotfiles/codex-session
git rm -r .config/codex-session/settings
git rm -r .config/codex-session/settings.bak.20260528
```

If `git rm -r` fails because the dir is untracked, fall back to `rm -rf`
then `git status`.

#### B2. Sweep for residual references

```bash
cd /home/gu/.dotfiles/codex-session
grep -rn "settings-layers\|settings_dir\|configs/profiles\|\[profiles\." \
  --exclude-dir=.git --exclude-dir=.agents 2>/dev/null
```

For each match outside `.agents/skills/`:

- If it's a comment / README / doc, update to the new vocabulary.
- If it's a config file (unexpected at this stage), fix it.
- If it's the `.gitignore`, leave it alone unless it mentions stale paths.

Read every non-skill file under `.config/codex-session/` end-to-end one
more time and confirm: no `settings/` references, no nested
`configs/profiles/` references, no legacy `[profiles.*]` blocks, no
`settings-layers:` keys.

#### B3. Commit blocks A + B together

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
sibling `profiles/<name>.config.toml` for per-profile overrides. Round
04 also added a codex-binary-version fail-fast gate (the wrapper
refuses to launch codex < 0.134.0).

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

### Block C — Cross-repo docs sync

#### C1. Discover codex-session-relevant docs in each external repo

```bash
# DocsNNotes
grep -rn "codex-session\|settings-layers\|configs/profiles\|\[profiles\." \
  ~/DocsNNotes 2>/dev/null \
  | grep -v "/\.git/" \
  | tee /tmp/round05-docsnotes-hits.txt

# Claude dotfiles
grep -rn "codex-session\|settings-layers\|configs/profiles" \
  ~/.dotfiles/claude 2>/dev/null \
  | grep -v "/\.git/" \
  | tee /tmp/round05-claude-hits.txt

# Claude-session dotfiles
grep -rn "codex-session\|settings-layers\|configs/profiles" \
  ~/.dotfiles/claude-session 2>/dev/null \
  | grep -v "/\.git/" \
  | tee /tmp/round05-claude-session-hits.txt
```

Triage each hit: codex-session-relevant → edit; unrelated → skip. Record
the edit list.

#### C2. Update `~/DocsNNotes/` codex-session-relevant docs

For each in-scope file (likely starting points:
`tech/tools/claude-code/codex-conventions.md` — touched in round 01 — and
any other doc in `tech/tools/{claude-code,codex}/`):

- Rewrite filesystem-layout sections to use sibling
  `profiles/<name>.config.toml`.
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

#### C3. Update `~/.dotfiles/claude/` codex-session-relevant content

If `/tmp/round05-claude-hits.txt` is empty, skip — no commit. Otherwise
edit only the in-scope files (codex-session-relevant) and commit:

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

#### C4. Update `~/.dotfiles/claude-session/` codex-session-relevant content

If `/tmp/round05-claude-session-hits.txt` is empty, skip — no commit.
Otherwise edit only the in-scope files and commit with the same template
as C3, swapping the repo name in the commit message.

### Final verification — manual host re-stow + smoke test

This is the user's responsibility, not the executor's. Tell the user in
the implementation report:

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

If the executor is running on a host where re-stowing has already
happened, run the smoke test directly and include the result in the
implementation report. If the executor cannot re-stow (devcontainer /
sandboxed environment), document that fact and rely on the in-repo
`just test` gates from round 04.

### Final Step: Update plan index

Update `.plan/01-todo/configs-rename-split-profiles/README.md` (in
`/workspaces/codex-session`):

1. In the `## Execution Order` table, find the row for round 05.
2. Change `Status` from `todo` to `done`.
3. Change `Completed` from `--` to today's date (`YYYY-MM-DD`).
4. In the README.md header blockquote, change `Status: todo` to
  `Status: done`.
5. Move the plan directory to done:

```bash
cd /workspaces/codex-session
mkdir -p .plan/02-done && mv .plan/01-todo/configs-rename-split-profiles .plan/02-done/configs-rename-split-profiles
```

(Include this directory move in a follow-up commit in
`/workspaces/codex-session`.)

## Acceptance Criteria

### Block A + B (in `~/.dotfiles/codex-session`)

- [ ] `.config/codex-session/settings/` directory no longer exists.
- [ ] `.config/codex-session/settings.bak.20260528/` directory no longer
    exists.
- [ ] `.config/codex-session/configs/` contains `base.toml`, `plugins.toml`,
    `projects.toml` — content matches the round-05 specs (no
    `profile = "..."` / `[profiles.*]`).
- [ ] `.config/codex-session/profiles/` (sibling of `configs/`) contains
    `deep.config.toml`, `fast.config.toml`, `ping.config.toml` with bare
    top-level keys.
- [ ] `config-recipes/default.yaml` uses `config-layers:` and lists `base`,
    `projects`, `plugins` in that order.
- [ ] `config.toml` comment header references `configs/` and sibling
    `profiles/`.
- [ ] `grep -rn "settings-layers\|settings_dir\|configs/profiles\|\[profiles\."
    --exclude-dir=.git --exclude-dir=.agents` returns no matches.
- [ ] A single commit in `~/.dotfiles/codex-session` captures blocks A + B
    with a Conventional Commit message citing codex v0.134+ and the
    sibling profiles layout.

### Block C (cross-repo)

- [ ] `~/DocsNNotes` has at least one updated codex-session-relevant doc
    (or, if the pre-scan found none stale, the implementation report
    documents that no updates were needed). Where updates happen, one
    Conventional Commit captures them.
- [ ] `~/.dotfiles/claude` and `~/.dotfiles/claude-session` are inspected;
    if updates are needed, each gets one Conventional Commit. If not,
    the report documents the no-op.
- [ ] No drive-by edits outside codex-session-relevant content.

### Plan index

- [ ] Plan `README.md` execution order table shows round 05 as `done`
    with today's date.
- [ ] Plan `README.md` header status is `done`.
- [ ] Plan directory moved from
    `.plan/01-todo/configs-rename-split-profiles` to
    `.plan/02-done/configs-rename-split-profiles`.

## Next Round

This is the final round.

After this round, the wrapper crate, its tests, all in-repo documentation,
the dotfiles source-of-truth, and the cross-repo docs in `~/DocsNNotes` and
`~/.dotfiles/{claude,claude-session}` are all aligned with codex v0.134+'s
split-profile input contract AND with the sibling `profiles/` input layout.
The composability contract codified in round 01 (CLAUDE.md § Codex config
compatibility) becomes the standing rule for future upstream config
changes. The codex-version fail-fast gate (round 04) ensures the wrapper
will refuse to launch a child binary that doesn't honor that contract.
