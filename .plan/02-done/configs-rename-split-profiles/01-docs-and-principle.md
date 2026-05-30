# Round 01: Docs & API-Compatibility Principle

> Plan: configs-rename-split-profiles | Round: 01 of 04 | Complexity: L
> Generated: 2026-05-28 | Repo: /workspaces/codex-session

## Context

Upstream codex CLI v0.134.0 (released 2026-05-26) hardened its profile config contract:

- `$CODEX_HOME/config.toml` must NOT contain `profile = "..."` or `[profiles.*]`.
- Per-profile overrides live in sibling files `$CODEX_HOME/<name>.config.toml` with **bare**
  top-level keys (no `[profiles.<name>]` header).
- Profile activation is `--profile <name>` on the CLI only. No in-file default selector.
- No backward-compat flag.

`codex-session` is a _composer_ — it reads layered TOML inputs and writes the `$CODEX_HOME/`
tree codex consumes. When codex changes its input contract, the composer must follow. The repo's
current docs describe the legacy model (`[profiles.<name>]`, `profile = "X"` selector) and do not
state the rule that the composer's output MUST match codex's input dialect byte-for-byte.

This round codifies that rule in `README.md`, `CLAUDE.md`, `docs/upstream-codex.md`, and the
cross-repo reference `~/DocsNNotes/tech/tools/claude-code/codex-conventions.md`. It writes the
contract down before any Rust code moves, so rounds 02–04 have a single citable source of truth.

No Rust code, no test changes, no `.config/` migrations in this round.

References:

- <https://developers.openai.com/codex/config-advanced#profiles>
- <https://developers.openai.com/codex/cli/reference> (`--profile` layers
  `$CODEX_HOME/<name>.config.toml` on top of base)
- <https://developers.openai.com/codex/changelog> (v0.134.0 profile restructuring)
- <https://github.com/openai/codex/releases/tag/rust-v0.134.0>

## Previous Rounds

This is the first round — no prior rounds.

## Scope of This Round

**In scope:**

- `README.md` — Filesystem-layout section refresh + new short "Composability contract" subsection.
- `CLAUDE.md` — new "Codex config compatibility" section after § Breaking Changes Policy.
- `docs/upstream-codex.md` — rewrite §F6b (`--profile` flag) to the v0.134+ contract; add §F6c
  documenting the legacy-form deprecation with a changelog quote; update path references in the
  "codex-session-specific notes" subsection from `settings.toml` to `configs.toml` (write-side
  vocabulary; on-disk file is renamed in round 03).
- `~/DocsNNotes/tech/tools/claude-code/codex-conventions.md` — rewrite the § Profile Strategy
  section, update path references from `settings/` to `configs/`, replace `[profiles.*]`-mental
  model with the split-file model.

**Out of scope (deferred to later rounds):**

- Any Rust code change (round 02).
- Any rename of on-disk paths or files (rounds 02–03).
- Any changes to test files (rounds 02–03).
- `~/.dotfiles/codex-session/` user-config migration (round 04).

## Current State

### Key Files (read these first)

- `/workspaces/codex-session/README.md` — Filesystem-layout block (≈ lines 51–82) currently
  references `settings/*.toml` and explains the manifest's `settings-layers:` field. The
  cache-layer section names `settings.toml`. There is no "composability contract" subsection.

  Excerpt (lines 56–82):

  ```text
  $XDG_CONFIG_HOME/codex-session/
    config.toml                           wrapper config
    config-recipes/*.yaml                       config-recipe manifests
    settings/*.toml                       settings layers

  $XDG_CACHE_HOME/codex-session/
    settings.toml                         trust / cache-layer writes
    quota/<account>.json                  cached quota responses
  ...
  ConfigRecipe manifests list ordered `settings-layers`. Each layer is parsed from
  `settings/<name>.toml`, deep-merged in order, stripped of its optional `[env]`
  table, then written into the session directory. Stock mode still creates a
  session directory with an empty `config.toml`.
  ```

- `/workspaces/codex-session/CLAUDE.md` — current sections: Quality gates, CLI Design System,
  Breaking Changes Policy (lines 41–52), Upstream codex behavior reference (lines 54–63). The
  new section lands directly after § Breaking Changes Policy.

- `/workspaces/codex-session/docs/upstream-codex.md` — §F6b (lines 123–135) currently reads:

  ```text
  ## F6b — `--profile` CLI flag

  Codex supports `-p, --profile <CONFIG_PROFILE>` to select a named
  configuration profile at runtime. The flag maps to a `[profiles.<name>]`
  section in `config.toml`. Model resolution precedence (highest to lowest):
  CLI `--model` → `-c model=` override → `--profile` section → top-level
  `model` → catalog default.

  - **Sources:** `codex --help` (verified 2026-05-26).
  - **Implementation note:** The heartbeat probe in
    `src/services/account/gate.rs` uses `--profile ping` with an isolated
    `CODEX_HOME` to select the probe model without `--model` hardcoding.
  ```

  The "codex-session-specific notes" subsection (lines 169–174) currently reads:

  ```text
  - **Cache layer is the trust persistence home.** `codex-session` writes
    trust decisions to `<XDG_CACHE_HOME>/codex-session/settings.toml`, which
    is loaded first by `compose()` (`src/services/config_recipe/mod.rs`). The
    stow-managed user layers in `<XDG_CONFIG_HOME>/codex-session/settings/`
    are **never** modified by the wrapper — that's the composeability
    contract.
  ```

- `/home/gu/DocsNNotes/tech/tools/claude-code/codex-conventions.md` — § Profile Strategy
  (lines 55–68) reads:

  ```text
  ## Profile Strategy

  Two stock Codex profiles are defined in `base.toml`:

  - **`deep`** (default) — no model override (inherits current catalog default), `high` effort. Used
    for planning and new-thread reasoning tasks. Calls omit `--profile`.
  - **`fast`** — `gpt-5.4-mini`, `medium` effort. Used for execution (implementation resume, code
    review rounds). Calls pass `--profile fast` on `exec` (before the `resume` subcommand if
    resuming).

  The wrapper renamed its own profile flag to `--config-recipe` (commit 3117d8e), so `--profile`
  passes through to stock Codex for selecting `[profiles.*]` tables. `ping` remains a
  health-probe-only profile.
  ```

  Also: line 11 names `~/.config/codex-session/settings/base.toml` as SoT. Update to
  `configs/base.toml`. Lines 419–421 reiterate `deep` is implied default — update to "calls omit
  `--profile` only when the user-config has set it externally; codex itself has no in-file
  default selector".

### Existing Patterns

- `docs/upstream-codex.md` follows an "F#" numbered sectioning. Add §F6c immediately after §F6b
  to keep the lineage of profile-related facts together.
- Each F# block ends with `**Sources:**` (URLs) and optionally `**Implementation note:**`.
- `Last verified` date is bumped per-section when content changes. Use today's date: `2026-05-28`.
- `README.md` uses prose-then-tables; the new "Composability contract" subsection follows the same
  style.
- `CLAUDE.md` sections are short (4–10 lines), declarative, and consult-first ("Consult X before
  doing Y").
- `DocsNNotes/codex-conventions.md` is verbose and example-heavy; mirror that style there.

## Implementation Steps

### Step 1: Add "Codex config compatibility" section to CLAUDE.md

Insert immediately after § Breaking Changes Policy (ends at line 52) and before § Upstream codex
behavior reference (line 54).

New section, verbatim:

```markdown
## Codex config compatibility

`codex-session` is a composer, not a config dialect. The `$CODEX_HOME/` tree it
emits (under `<state>/accounts/<acct>/groups/<group>/`) MUST be byte-for-byte
structurally compatible with what upstream `codex` accepts as input. Composability
lives at the input layer (`configs/` directory + recipe manifests), never at the
output layer.

Concretely:

- If upstream codex rejects a key shape (e.g. legacy `profile = "..."` selector
  or `[profiles.*]` tables in `config.toml` since v0.134.0), the wrapper rejects
  it too — at both input layers (`configs/*.toml`) and emitted output. No
  compat shim, no alias, no auto-migration.
- Profile overrides emit as sibling files `$CODEX_HOME/<name>.config.toml` with
  bare top-level keys. Source-of-truth: [docs/upstream-codex.md](./docs/upstream-codex.md)
  §F6b.
- When upstream codex changes its config contract, update `docs/upstream-codex.md`
  first, then mirror the change in the composer.
```

### Step 2: Rewrite README.md filesystem layout + add composability subsection

In `README.md`, replace the existing filesystem-layout block (lines 51–82) with:

````markdown
## Filesystem layout

Run `codex-session config status` or `codex-session doctor` to see resolved
paths for the current environment. The general structure:

```text
$XDG_CONFIG_HOME/codex-session/
  config.toml                           wrapper config
  config-recipes/*.yaml                 config-recipe manifests
  configs/                              composable config layers
    *.toml                              base config layers
    profiles/
      <name>.config.toml                profile overrides (one file per profile)

$XDG_CACHE_HOME/codex-session/
  configs.toml                          trust / cache-layer writes
  quota/<account>.json                  cached quota responses

$XDG_STATE_HOME/codex-session/
  state/last-account                    LRU pointer (plain text)
  thread-index.jsonl                    cross-account session resume index (JSONL)
  accounts/<account>/
    auth.json                           account seed (auth source of truth)
    cooldown.json                       failover cooldown state
    groups/<group-id>/
      auth.json                         session copy (synced back on exit)
      config.toml                       composed codex base config (no profile keys)
      <name>.config.toml                emitted per-profile sibling files
      .codex-session-compose.json       composition metadata
      session-meta.json                 session metadata
```

ConfigRecipe manifests list an ordered `config-layers:` array. Each layer is
parsed from `configs/<name>.toml`, deep-merged in order, stripped of its
optional `[env]` table, then written into the session directory as `config.toml`.
Profile overrides are emitted as sibling `<name>.config.toml` files, copied
1:1 from `configs/profiles/<name>.config.toml`. Stock mode still creates a
session directory with an empty `config.toml`.

## Composability contract

codex-session's emitted `$CODEX_HOME/` tree is byte-for-byte structurally
compatible with upstream codex's native input contract. Composability is
layered on top, never instead of:

- The emitted `config.toml` matches upstream codex's expected base config —
  it contains NO legacy `profile = "..."` selector and NO `[profiles.*]`
  tables (rejected by codex v0.134+).
- Profile overrides emit as sibling `<name>.config.toml` files, selected by
  `codex --profile <name>` at invocation time.
- The wrapper never injects `--profile`. Users pass it on the CLI and it
  flows to codex unchanged.

If you want a layer to apply only when a specific profile is active, put it
under `configs/profiles/<name>.config.toml`. If you want it to apply
unconditionally, put it under `configs/<layer>.toml`.
````

### Step 3: Rewrite docs/upstream-codex.md §F6b and add §F6c

Replace lines 123–135 (§F6b) with:

```markdown
## F6b — `--profile` CLI flag (v0.134+ contract)

Codex supports `-p, --profile <CONFIG_PROFILE>` to select a named profile at
runtime. Since v0.134.0 the flag overlays the file
`$CODEX_HOME/<profile>.config.toml` on top of the base `$CODEX_HOME/config.toml`.
Per-profile files contain **bare top-level keys** — there is no
`[profiles.<name>]` header. The base `config.toml` must contain no top-level
`profile = "..."` selector and no `[profiles.*]` table. There is no in-file
default selector; `--profile` is the only way to activate one.

Model resolution precedence (highest to lowest): CLI `--model` →
`-c model=` override → `--profile`-overlaid file → top-level `config.toml` →
catalog default.

- **Sources:** [Advanced Configuration §Profiles](https://developers.openai.com/codex/config-advanced#profiles),
  [CLI reference (`--profile`)](https://developers.openai.com/codex/cli/reference),
  [openai/codex release v0.134.0](https://github.com/openai/codex/releases/tag/rust-v0.134.0).
  `Last verified`: 2026-05-28.
- **Implementation note:** The heartbeat probe in `src/services/account/gate.rs`
  uses `--profile ping` with an isolated `CODEX_HOME` containing a base
  `config.toml` plus a sibling `ping.config.toml` (the contents of
  `configs/profiles/ping.config.toml` copied 1:1). codex-session itself never
  injects `--profile` for user-facing exec calls.

## F6c — Legacy profile form rejection (v0.134+ breaking change)

Codex v0.134.0 stopped accepting two legacy forms inside `$CODEX_HOME/config.toml`:

1. The top-level selector `profile = "<name>"`.
2. The nested table `[profiles.<name>]`.

Either form triggers a hard error pointing at
<https://developers.openai.com/codex/config-advanced#profiles> with migration
guidance ("move those settings into `<name>.config.toml` and remove the legacy
profile selector/table"). There is NO backward-compat flag or environment
variable to re-enable the legacy form.

This is a load-bearing fact for `codex-session`: the composer must (a) reject
the legacy form at the input layer (`configs/*.toml`), (b) never emit it in
`$CODEX_HOME/config.toml`. See §F6b for the new contract and
[CLAUDE.md § Codex config compatibility](../CLAUDE.md) for the wrapper rule.

- **Sources:** [Codex changelog](https://developers.openai.com/codex/changelog)
  (v0.134.0 entry), [release v0.134.0](https://github.com/openai/codex/releases/tag/rust-v0.134.0).
  `Last verified`: 2026-05-28.
```

In the same file, in the "codex-session-specific notes" subsection (lines 169–174), replace the
"Cache layer is the trust persistence home" bullet with:

```markdown
- **Cache layer is the trust persistence home.** `codex-session` writes
  trust decisions to `<XDG_CACHE_HOME>/codex-session/configs.toml`, which
  is loaded first by `compose()` (`src/services/config_recipe/mod.rs`). The
  stow-managed user layers in `<XDG_CONFIG_HOME>/codex-session/configs/`
  (including `configs/profiles/*.config.toml`) are **never** modified by
  the wrapper — that's the composeability contract.
```

### Step 4: Update ~/DocsNNotes/tech/tools/claude-code/codex-conventions.md

Read `/home/gu/DocsNNotes/tech/tools/claude-code/codex-conventions.md` first to confirm current
line numbers. Then apply the following edits.

Replace the SoT line near line 11:

- **From:** `` `~/.config/codex-session/settings/base.toml`. ``
- **To:** `` `~/.config/codex-session/configs/base.toml`. ``

Replace § Profile Strategy (lines 55–68) with:

```markdown
## Profile Strategy

Profile overrides live as separate files under
`~/.config/codex-session/configs/profiles/<name>.config.toml` (codex v0.134+
contract). Each file contains bare top-level keys, no `[profiles.<name>]`
header. codex-session emits these 1:1 as siblings of the base `config.toml` in
the session's `$CODEX_HOME/`.

Stock profiles used by skills:

- **`deep`** — high reasoning effort, full sandbox. Used for planning and
  new-thread reasoning. Calls omit `--profile`; codex uses the base
  `config.toml` alone. If the user wants `deep` as a personal default, they
  must pass `--profile deep` explicitly on the CLI — codex itself no longer
  supports an in-file default selector.
- **`fast`** — `gpt-5.4-mini`, `medium` effort. Used for execution
  (implementation resume, code review rounds). Calls pass `--profile fast`
  on `exec` (before the `resume` subcommand if resuming).
- **`ping`** — health-probe-only profile, used internally by `account health`.

The wrapper's own composability lever is `--config-recipe` (commit 3117d8e).
The `--profile` flag passes through to stock codex unchanged. codex-session
never injects `--profile`.

References: `docs/upstream-codex.md` §F6b–§F6c (codex-session repo);
<https://developers.openai.com/codex/config-advanced#profiles>.
```

Also replace the bullet near lines 419–421 ("Planning and new-thread calls inherit the default
`deep` profile (no `--profile` flag).") with:

```markdown
- Planning and new-thread calls omit `--profile`, so codex uses only the base
  `config.toml` (no profile overlay). Execution and review calls must pass
  `--profile fast` on `exec` (before the `resume` subcommand if resuming).
  Codex v0.134+ has no in-file default-profile selector — the wrapper never
  injects one.
```

Spot-check the rest of the file for stray `settings/*.toml`, `[profiles.<name>]`, or `~/.codex/`
references and update each to the new vocabulary; report any non-trivial deviation in the
implementation summary.

### Final Step: Update plan index

Update `.plan/01-todo/configs-rename-split-profiles/README.md`:

1. In the `## Execution Order` table, find the row for round 01.
2. Change `Status` from `todo` to `done`.
3. Change `Completed` from `--` to today's date (`YYYY-MM-DD`).

## Acceptance Criteria

- [ ] `CLAUDE.md` has a new "Codex config compatibility" section between § Breaking Changes Policy
      and § Upstream codex behavior reference, with the API-compat rule and the link to
      `docs/upstream-codex.md` §F6b.
- [ ] `README.md` filesystem layout shows `configs/`, `configs/profiles/<name>.config.toml`, and
      `configs.toml` (cache); the manifest field is named `config-layers:`; a new "Composability
      contract" subsection follows the layout block.
- [ ] `docs/upstream-codex.md` §F6b is rewritten to the v0.134+ contract with the correct
      precedence chain and `Last verified: 2026-05-28`. A new §F6c documents the legacy-form
      rejection with the changelog source link.
- [ ] `docs/upstream-codex.md` codex-session-specific bullet about the cache layer references
      `configs.toml` and `configs/` (write-side vocabulary; on-disk file is renamed in round 03).
- [ ] `~/DocsNNotes/tech/tools/claude-code/codex-conventions.md` § Profile Strategy is rewritten
      to describe split-file profiles, the SoT line near line 11 points at `configs/base.toml`,
      and the implied-`deep`-default text in § Safety Rules is corrected.
- [ ] No Rust source files modified in this round.
- [ ] No test files modified in this round.
- [ ] `just lint` passes (markdown is touched but no Rust → fmt-check + clippy-strict + print
      ownership lint remain clean).
- [ ] Plan `README.md` execution order table shows round 01 as `done` with today's date.

## Next Round

Round 02 — Composer & input rename — picks up with all four docs already stating the contract.
The Rust changes in round 02 (rename `settings_dir` → `configs_dir`, split-emit in
`write_session_artifacts`, add `config-layers` manifest field, add legacy-form rejection) will
quote the §F6b/§F6c text written here in their error messages and doc-comments.
