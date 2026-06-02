# configs/ Rename + Split-Profile Emission for Codex 0.134+

> Complexity: L | Rounds: 5 | Generated: 2026-05-28 | Repo: /workspaces/codex-session
> Status: done

## Problem Statement

Upstream codex CLI v0.134.0 (released 2026-05-26) hardened its profile config contract:

- `$CODEX_HOME/config.toml` must **not** contain a top-level `profile = "..."` selector or any
  `[profiles.*]` table.
- Per-profile overrides live in sibling files `$CODEX_HOME/<name>.config.toml` with **bare**
  top-level keys (no `[profiles.<name>]` header).
- Profile activation is `--profile <name>` on the CLI only — no in-file default selector exists
  anymore.
- No backward-compat flag. The legacy form is rejected with a hard error pointing at this contract:
  <https://developers.openai.com/codex/config-advanced#profiles>.

`codex-session`'s composer (`src/services/config_recipe/composition.rs::write_session_artifacts`)
currently merges all `settings/*.toml` input layers and writes them verbatim into the session
directory's single `config.toml`. The merged output still carries `profile = "deep"` and
`[profiles.{deep,fast,ping}]` tables — verified on disk at
`~/.local/state/codex-session/sessions/pid-131710/config.toml`. Every invocation that flows
through `pass_through` or `account health` now fails with codex 0.134+'s legacy-form rejection.

This plan refits codex-session so its emitted `$CODEX_HOME/` tree is byte-for-byte
structurally compatible with codex's new input contract, and codifies the principle that the
wrapper's output dialect MUST track upstream codex's input dialect.

## Strategy

Five sequential rounds, foundations-first:

1. **Docs & principle** — codify the API-compatibility rule in `README.md`, `CLAUDE.md`,
   `docs/upstream-codex.md` (§F6b rewrite + new §F6c), and the cross-repo
   `~/DocsNNotes/tech/tools/claude-code/codex-conventions.md`. No code changes. Establishes the
   contract the next four rounds enforce.
2. **Composer & input rename** — rename `settings/` → `configs/` everywhere in the Rust crate,
   rename the manifest YAML field, add legacy-form rejection at layer read, modify
   `write_session_artifacts` to emit `<name>.config.toml` siblings, add `profile-files:`
   manifest support, update tests for composer + config-recipe surface.
3. **Heartbeat probe, cache rename, doctor** — `extract_ping_config` reads
   `configs/profiles/ping.config.toml` directly; `heartbeat_probe` writes `ping.config.toml`
   under the probe CODEX_HOME; cache file renamed to `configs.toml`; doctor messages updated and
   legacy-form detection added.
4. **Codex v0.134+ fail-fast gate + sibling `profiles/` restructure** — Block A adds a
   pre-launch gate that probes `codex --version`, parses it, and refuses to launch any child
   older than 0.134.0, with the same check mirrored as a doctor finding so users can diagnose
   the version skew without running a pass-through. Block B promotes `profiles_dir` from a
   derived `configs_dir.join("profiles")` method to a first-class field defaulting to
   `config_dir.join("profiles")` (sibling of `configs/`, not nested); updates every consumer,
   fixture, and in-repo doc that round 01–03 wrote with the nested assumption (including
   `README.md`, `docs/upstream-codex.md`, and the doctor sweep that flags the obsolete nested
   `configs/profiles/` layout). One commit in `/workspaces/codex-session`.
5. **Dotfiles propagation + cleanup + cross-repo docs sync** — migrate
   `~/.dotfiles/codex-session/.config/codex-session/` to the sibling layout (`settings/` →
   `configs/` with sibling `profiles/<name>.config.toml`), update the `default.yaml` manifest,
   sweep `~/.dotfiles/codex-session` for obsolete leftovers to keep it lean, and finally sync
   codex-session-relevant docs across `~/DocsNNotes` and `~/.dotfiles/{claude,claude-session}`.
   Commit in each affected repo separately.

Why this order: principle first (round 01) so rounds 02–05 have a single source of truth to
cite. Composer (round 02) before consumers (round 03) so the heartbeat probe can rely on the new
emission. The codex-binary version gate is grouped with the sibling-`profiles/` restructure in
round 04 because both are wrapper-crate-only changes that finalize the contract before any
external repos move; doing the gate first means every fixture in the restructure can assume the
new contract is in force. The dotfiles propagation, cleanup, and cross-repo docs sync are
separated into round 05 because they touch disjoint repos with independent acceptance criteria
and validate the whole chain end-to-end on a real host.

## Execution Order

| Round | File                                             | Topic                                                            | Status | Completed  |
| ----- | ------------------------------------------------ | ---------------------------------------------------------------- | ------ | ---------- |
| 01    | `01-docs-and-principle.md`                       | Codify API-compat principle in README/CLAUDE/upstream/DocsNNotes | done   | 2026-05-28 |
| 02    | `02-composer-and-input-rename.md`                | Rename settings→configs, split-emit, manifest field, validation  | done   | 2026-05-28 |
| 03    | `03-heartbeat-cache-doctor.md`                   | Ping probe rewrite, cache file rename, doctor detection          | done   | 2026-05-28 |
| 04    | `04-codex-compat-and-sibling-profiles.md`        | Codex v0.134+ fail-fast gate + sibling `profiles/` restructure   | done   | 2026-05-28 |
| 05    | `05-dotfiles-propagation-and-cross-repo-sync.md` | Dotfiles propagation + cleanup + cross-repo docs sync            | done   | 2026-05-28 |

## Execution Commands

```bash
# Execute a single round (do this once per round, in order):
/prex -ar .plan/01-todo/configs-rename-split-profiles/01-docs-and-principle.md
/prex -ar .plan/01-todo/configs-rename-split-profiles/02-composer-and-input-rename.md
/prex -ar .plan/01-todo/configs-rename-split-profiles/03-heartbeat-cache-doctor.md
/prex -ar .plan/01-todo/configs-rename-split-profiles/04-codex-compat-and-sibling-profiles.md
/prex -ar .plan/01-todo/configs-rename-split-profiles/05-dotfiles-propagation-and-cross-repo-sync.md

# Or point at the directory — /prex reads the execution order table and picks the next todo round:
/prex -ar @.plan/01-todo/configs-rename-split-profiles/
```

## Execution Discipline

**Rounds must be executed one at a time.** Each round is a self-contained unit of work designed for
a single `/prex` session. Do not attempt to implement multiple rounds in one session.

After completing a round:

1. Consult the **Execution Order** table above.
2. Find the next round with status `todo`.
3. Execute it in a **fresh** `/prex` session.
4. Repeat until all rounds show status `done`.

Pointing `/prex` at the directory or this README selects ONE round (the next `todo`), executes it,
then stops. It does NOT proceed through the table in a single session. Fresh sessions prevent
context contamination between rounds, keep token usage predictable, and let you review intermediate
results before proceeding.

## Decisions & Constraints

- **Wrapper is a composer, not a config dialect.** codex-session's emitted `$CODEX_HOME/` tree
  must be byte-for-byte structurally compatible with codex's native input contract. Composability
  is layered on top, never instead of. If upstream drops a key shape, the wrapper drops it too.
  This principle is codified in round 01.
- **Profile activation: forward-only.** The wrapper never injects `--profile`. Users pass it on
  the CLI and it flows to codex unchanged. No `default-profile` field is added to the manifest or
  to wrapper config. Mirrors codex's own model (no in-file default selector exists upstream).
- **Profile composition: 1:1 emit.** Each `configs/profiles/<name>.config.toml` is copied verbatim
  to `$CODEX_HOME/<name>.config.toml`. No deep-merge across layers. Matches codex's input model
  exactly.
- **Vocabulary rename:** `settings/` → `configs/` everywhere. Manifest YAML field `settings-layers:`
  → `config-layers:`. Cache file `$XDG_CACHE_HOME/codex-session/settings.toml` →
  `configs.toml`.
- **`profiles/` is a sibling of `configs/`, not nested.** Round 02 introduced the nested
  `configs/profiles/<name>.config.toml` shape as an intermediate. Round 04 promotes
  `profiles_dir` to a first-class field defaulting to `config_dir.join("profiles")`. Rationale:
  `configs/` and `profiles/` are two distinct input axes (base layers vs. per-profile
  overrides) and the wrapper config should make that visible at the top level rather than hide
  it behind a subdirectory. Doctor flags any remaining nested `configs/profiles/` dir as a
  migration finding.
- **Codex version pin: forward-only.** The wrapper refuses to launch a codex child older than
  the version that hardened the config contract codex-session targets (currently `0.134.0`).
  The pin lives in one place (`src/codex_compat.rs::REQUIRED_CODEX_VERSION`) and is enforced
  at two points: a pre-launch gate on every code path that invokes codex (cached via
  `LazyChild`, so it's free after the first call), and a mirrored `doctor` check
  (`check_codex_version_minimum`). Pre-release suffixes of the required release
  (e.g. `0.134.0-rc1`, `0.134.0-alpha.1`) are treated as `Ok`. Unparsable version output is
  `Warn`, not `Fail`. Rationale: prevents a confusing handoff where codex rejects our
  emitted file instead of the wrapper rejecting the version skew. Placed at pre-child-invocation
  (not in `main()`) so `codex-session doctor` / `--version` / `config-recipe …` still work
  against an older codex and can diagnose the version skew.
- **Cross-repo docs scope (round 05):** the cross-repo docs sweep updates only
  codex-session-relevant content in `~/DocsNNotes` and
  `~/.dotfiles/{codex-session,claude,claude-session}` — unrelated Claude/Claude-session
  config and skill bundles are out of scope. Each repo gets its own commit.
- **Pre-v1.0 clean break.** Per `CLAUDE.md` § Breaking Changes Policy: no compat shim, no dual-name
  support, no auto-migration of legacy on-disk files. Doctor surfaces a clear error pointing at
  the new layout if either the old dir name or the legacy `[profiles.*]` form is detected.
- **Per-host migration is manual.** Round 05 updates the dotfiles repo. On each host where the
  user has stowed the old `settings/` tree, they manually re-stow after pulling. Doctor's
  legacy-detection error guides them.

## Rejected Alternatives

- **Inject `--profile <default>` from a manifest field.** Rejected. Codex itself no longer has an
  in-file default-profile mechanism. Adding one in the wrapper would re-introduce the same
  ambiguity codex deliberately removed, and the wrapper's principle (round 01) is to match codex's
  contract, not extend it.
- **Layered/composable profile files.** Rejected. Recipe-declared profile-layer maps add a second
  composition axis with no clear payoff: profile files are short, recipe-specific, and the
  wrapper still has full `config-layers` composability for the base config. 1:1 emit is simpler
  and codex-native.
- **Keep cache file as `settings.toml` for backward compatibility.** Rejected. Pre-v1.0 breaking
  changes policy explicitly disallows compat layers for renamed wrapper behavior. Vocabulary
  consistency wins.
- **Auto-migrate legacy `[profiles.*]` from user `configs/<layer>.toml` into separate profile
  files at compose time.** Rejected. Silent migration hides the contract change from the user
  and risks misinterpreting which keys are profile-specific vs. base-level. Doctor's
  legacy-detection error is explicit and actionable.

## Risks & Edge Cases

- **Heartbeat probe regression.** `extract_ping_config` is on the hot path for every
  `--account auto` invocation (via `account health`). Round 03 must keep behavior bit-exact: same
  `--profile ping` invocation, same isolated CODEX_HOME, same model resolution semantics —
  only the on-disk layout changes. Test coverage in `tests/account_health_cli.rs` is the
  guardrail.
- **Trust-sync path coupling.** The cache file rename (`settings.toml` → `configs.toml`) touches
  `pass_through.rs`, `trust_sync.rs`, and every test that round-trips trust state. Round 03
  isolates this to one round so the breakage surface is contained.
- **Doctor false-positive on existing `settings/` dirs.** Until users re-stow, their host will
  still have an old `settings/` dir alongside the new `configs/` dir. Round 02's doctor change
  surfaces this as a guided error (not a silent ignore) so the migration is visible.
- **Cross-repo coordination.** Round 05 touches three separate external git repos:
  `~/.dotfiles/codex-session` (layout migration + cleanup), `~/DocsNNotes` (cross-repo doc
  sync), and `~/.dotfiles/{claude,claude-session}` (codex-session-relevant doc sync). The
  executor must `cd` into each repo explicitly and make one commit per repo. Round 05 spells
  out the order and the per-repo commit boundaries. The wrapper-crate work (round 04) is in
  `/workspaces/codex-session` and lands one commit ahead of the external sweep.
- **Sibling restructure churn in round 04.** Promoting `profiles_dir` to a first-class field
  touches `ConfigRecipeConfig`, `FileConfigRecipeConfig`, `ConfigRecipePaths`, every
  constructor of `ConfigRecipePaths`, the composer's profile-file collection in
  `services/config_recipe/mod.rs`, every test fixture that wrote profile files under
  `configs/profiles/`, the doctor's legacy-form sweep (added in round 03 against
  `configs/profiles/` — now reads from `profiles_dir` and gets a new finding for any
  surviving nested layout), and every doc paragraph in `README.md` /
  `docs/upstream-codex.md` that names the nested path. Round 04 isolates this churn to one
  prex session so the diff is reviewable as a unit.
- **Codex `--version` output drift.** The classifier in `src/codex_compat.rs` tolerates both
  `codex` and `codex-cli` prefixes and pre-release suffixes; an unparsable string produces
  a `Warn` (not `Fail`) so the wrapper continues to operate against odd-but-likely-fine
  builds. `doctor` surfaces the same finding so the user has a single place to see when the
  parser failed. If upstream codex changes its `--version` output shape (e.g. multi-line or
  JSON), the classifier and the unit-test fixture both need updating — caught in round 04's
  `just test` gate against the dev-container's installed codex.
- **Note on `.plan/` tracking.** This repo's `.gitignore` does NOT list `.plan/`, and prior plan
  files (`.plan/01-todo/prex-sandbox-fix-tmpdir-migration/`) are committed. This plan follows the
  existing convention — plan files will be staged with the implementation commits.

## Completion

When all rounds are done:

```bash
# README header status set to "done" and completion timestamps filled in the table above.
mv .plan/01-todo/configs-rename-split-profiles .plan/02-done/configs-rename-split-profiles
```
