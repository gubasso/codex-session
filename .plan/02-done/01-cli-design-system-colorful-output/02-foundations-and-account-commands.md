# Round 02: Foundations & Account Commands

> Plan: cli-design-system-colorful-output | Round: 02 of 03 | Complexity: L | Generated: 2026-05-26
> Repo: /workspaces/codex-session

## Context

codex-session is a CLI wrapper around OpenAI's `codex` binary. Its user-facing output varies
wildly: `account quota` has colored bars, human times, and conditional ANSI styling (the gold
standard), while `account list` dumps raw key=value pairs, `health` and `cooldown` have plain
tables with no color, and mutations (add/use/remove/refresh) print plain text without `--format
json` support.

A CLI Design System specification now exists at `docs/design/cli-style-guide.md` (created in
Round 01). It defines the complete color palette, table layout rules, status indicators, symbols,
and per-command target output. This round applies that spec to the account command family and
establishes the shared style foundations.

## Previous Rounds

**Round 01** created:
- `docs/design/cli-style-guide.md` — the authoritative CLI design system specification
- Updated `CLAUDE.md` with a pointer to the spec and pre-v1.0 breaking changes policy

The spec defines: color palette (BOLD, DIM, BOLD_CYAN, GREEN, BOLD_GREEN, BOLD_YELLOW, RED,
BOLD_RED), table layout rules (DIM headers, dynamic widths, ▸ marker), status indicator mappings,
symbol usage (▸ ✓ ✗), time formatting rules, and per-command target layouts.

## Scope of This Round

**IN scope:**
- Rename `quota_styles` module to `styles` in `src/ui/mod.rs`
- Add `GREEN` constant to the styles module
- Rewrite `write_account_list` text output to use colored table with ▸ marker
- Colorize `write_account_current` with ▸ prefix and BOLD_CYAN
- Colorize `write_health_table` and `write_health_verbose` with semantic colors
- Colorize `write_account_cooldowns` with semantic colors
- Colorize `write_account_mutation` and add `--format json` support
- Migrate cooldown `--json` flag to `--format json` (breaking change)
- Add `--format` to any account subcommand that lacks it
- Update snapshot tests for help text changes

**OUT of scope:**
- `account quota` (already styled — the gold standard)
- `doctor`, `config status`, `config-recipe`, `version`, error rendering (Round 03)
- Changes to the design system spec itself
- New dependencies

## Current State

### Key Files

- `/workspaces/codex-session/src/ui/mod.rs` — Main UI module (917 lines). The `quota_styles`
  module at line 661 holds color constants used by quota rendering. The `style_open()` and
  `style_close()` helper functions at lines 681-695 gate ANSI output. Key methods to modify:
  - `write_account_list` (line 330) — raw dump, no color
  - `write_account_current` (line 365) — plain text `{name} ({source})`
  - `write_account_health` (line 450) → calls `write_health_table` (line 607) and
    `write_health_verbose` (line 541) — tables but no color
  - `write_account_cooldowns` (line 470) — table but no color
  - `write_account_mutation` (line 378) — plain text, no `--format json`

- `/workspaces/codex-session/src/cli/account.rs` — Account CLI arg structs. Contains
  `AccountCooldownShowArgs` with a `json: bool` field that needs migration to
  `format: OutputFormat`. Also contains `AccountAddArgs`, `AccountUseArgs`, `AccountRemoveArgs`,
  `AccountRefreshArgs` — none of which have `format` support yet.

- `/workspaces/codex-session/src/commands/account/cooldown.rs` — Cooldown command handler. Uses
  `args.json` to decide output format. Needs migration to `args.format`.

- `/workspaces/codex-session/src/commands/account/add.rs` — Add command handler. Calls
  `ctx.ui.write_account_mutation("added", ...)` without format support.

- `/workspaces/codex-session/src/commands/account/use_.rs` — Use command handler. Same pattern.

- `/workspaces/codex-session/src/commands/account/remove.rs` — Remove command handler. Same
  pattern.

- `/workspaces/codex-session/src/commands/account/refresh.rs` — Refresh command handler. Same
  pattern.

- `/workspaces/codex-session/src/commands/account/mod.rs` — Contains view structs including
  `AccountMutationView` (needs `verb` field for JSON output), `AccountListEntryView` (has
  `has_auth`, `last_used_at_unix`, `current` fields that will be colorized),
  `AccountCooldownEntryView`, `AccountHealthEntryView`.

### Existing Patterns

The `quota_styles` module name is misleading — its constants are used by all renderers, not just
quota. References to `quota_styles::` appear ~15 times in `src/ui/mod.rs`.

The existing color application pattern (from quota rendering):
```rust
let c = color::should_color(color::Stream::Stdout);
write!(stdout, "{}text{}", style_open(SOME_STYLE, c), style_close(SOME_STYLE, c))?;
```

`OutputFormat` enum is defined in `/workspaces/codex-session/src/cli/mod.rs`:
```rust
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, ValueEnum)]
pub(crate) enum OutputFormat {
    #[default]
    Text,
    Json,
}
```

The global `--format` flag exists on `GlobalArgs` but is `Option<OutputFormat>`. Some commands use
the global, others have per-command format args. The account subcommands that already have format
support (list, current, quota, health) get it from their own args struct or the global.

## Implementation Steps

### Step 1: Rename `quota_styles` → `styles` and add `GREEN`

**File:** `/workspaces/codex-session/src/ui/mod.rs`

1. Rename the module declaration at line 661 from `mod quota_styles` to `mod styles`
2. Add to the module:
  ```rust
  pub(super) const GREEN: Style = Style::new()
      .fg_color(Some(anstyle::Color::Ansi(AnsiColor::Green)));
  ```
3. Find-replace all `quota_styles::` → `styles::` in the file (~15 occurrences)

This is a mechanical rename with one new constant.

### Step 2: Rewrite `write_account_list` text output

**File:** `/workspaces/codex-session/src/ui/mod.rs` — method at line 330

Replace the raw dump with a colored table matching the design system spec:

```text
  ACCOUNT    AUTH   LAST USED      STATUS
▸ cwnt       ✓      2m ago         active (lru)
  default    ✓      1d 14h ago
```

Implementation:
- Compute `use_color` from `color::should_color(color::Stream::Stdout)`
- Compute dynamic column width from max account name length (minimum 7 for "ACCOUNT")
- Print DIM header row: `ACCOUNT`, `AUTH`, `LAST USED`, `STATUS`
- For each account:
  - If current: prefix with `▸` in BOLD_CYAN, name in BOLD
  - Else: prefix with two spaces, name in BOLD
  - Auth: `✓` in GREEN if `has_auth`, `✗` in RED if not
  - Last used: `human_age(last_used_at_unix)` if Some, `—` in DIM if None
  - Status column: only for current account — `"active ({source})"` in BOLD_CYAN
- JSON branch stays unchanged (already works)

### Step 3: Colorize `write_account_current`

**File:** `/workspaces/codex-session/src/ui/mod.rs` — method at line 365

Change from:
```rust
writeln!(stdout, "{} ({})", view.name, view.source)
```
To styled output:
```text
▸ cwnt (lru)
```

- `use_color` from `color::should_color(color::Stream::Stdout)`
- `▸` marker in BOLD_CYAN; name in BOLD (matches the style guide SoT)
- `({source})` in DIM (secondary metadata)

### Step 4: Colorize `write_health_table`

**File:** `/workspaces/codex-session/src/ui/mod.rs` — function at line 607

Add `use_color` parameter (from `write_account_health` which calls it). Apply semantic colors per
the design system:

- Header row: DIM
- Account name for active entry: BOLD
- Token: `"ok"` → BOLD_GREEN, `"invalid"` → BOLD_RED, contains `"unknown"` → BOLD_YELLOW
- Status: `"live"` → BOLD_GREEN, `"cache_only"` → BOLD_YELLOW, `"cache_missing"` → BOLD_RED
- Active `true`: BOLD_CYAN
- Cooldown `true`: BOLD_RED
- Fetched timestamp: convert to `human_age()` (already done), style DIM

Modify `write_account_health` to pass `use_color` down to `write_health_table`.

### Step 5: Colorize `write_health_verbose`

**File:** `/workspaces/codex-session/src/ui/mod.rs` — function at line 541

Similar treatment to the table: add `use_color`, colorize the key values using the same semantic
mapping. The verbose output is key-value pairs, not a table, so apply color to the values:
- `token:` value colored by content (ok/invalid/unknown)
- `status:` value colored by content (live/cache_only/cache_missing)
- `active:` value colored BOLD_CYAN if true
- `cooldown:` value colored BOLD_RED if true

Modify `write_account_health` to pass `use_color` to `write_health_verbose` as well.

### Step 6: Colorize `write_account_cooldowns`

**File:** `/workspaces/codex-session/src/ui/mod.rs` — method at line 470

- Header `"ACCOUNT     STATUS       RESETS         REASON"` → DIM
- Account name → BOLD
- Status `"eligible"` → BOLD_GREEN, `"cooled-down"` → BOLD_RED
- Reset countdown → BOLD_YELLOW (when present)
- `use_color` from `color::should_color(color::Stream::Stdout)`

### Step 7: Migrate cooldown `--json` to `--format json`

**Files:**
- `/workspaces/codex-session/src/cli/account.rs` — Change `AccountCooldownShowArgs`: replace
  `json: bool` with `format: OutputFormat` using `#[arg(long, value_name = "FMT", value_enum,
  default_value_t = OutputFormat::Text)]`
- `/workspaces/codex-session/src/commands/account/cooldown.rs` — Replace `if args.json {
  OutputFormat::Json } else { OutputFormat::Text }` logic with direct `args.format`

Clean break, no hidden alias.

**Also delete the "known migration gap" note in `CLAUDE.md`.** The committed Breaking
Changes Policy section currently documents this exact flag as deferred:
"`account cooldown show` still exposes a bare `--json` flag that will move to
`--format json` in a later round." Once this step lands, that sentence is false —
remove it in the same change so `CLAUDE.md` stays accurate.

### Step 8: Add `--format json` to mutation commands

**Files to modify:**
- `/workspaces/codex-session/src/cli/account.rs` — Add `format: OutputFormat` field to
  `AccountAddArgs`, `AccountUseArgs`, `AccountRemoveArgs`, `AccountRefreshArgs`
- `/workspaces/codex-session/src/ui/mod.rs` — Change `write_account_mutation` signature to accept
  `format: OutputFormat`. In JSON mode, serialize the view. In text mode, apply colors.
- `/workspaces/codex-session/src/commands/account/mod.rs` — `write_account_mutation`
  already receives the verb as a separate `verb: &'static str` parameter (see
  `src/ui/mod.rs`), so text output needs no struct change. For JSON output, add a
  `verb` field to `AccountMutationView` so the serialized object includes it (the
  view currently holds only `name` and `path`).
- `/workspaces/codex-session/src/commands/account/add.rs` — Pass `args.format` to the UI method
- `/workspaces/codex-session/src/commands/account/use_.rs` — Same
- `/workspaces/codex-session/src/commands/account/remove.rs` — Same
- `/workspaces/codex-session/src/commands/account/refresh.rs` — Same

Colorized text output:
```text
✓ account added: cwnt
  path: /home/gu/.local/state/codex-session/accounts/cwnt
```
- `✓` in GREEN
- `account {verb}:` in BOLD_GREEN
- account name in BOLD
- `path:` label in DIM
- path value in DIM

### Step 9: Update snapshot tests

**Files:**
- All snapshot files under `tests/snapshots/` that capture help text for account subcommands

Run `just test` and update snapshots with `cargo insta review`. The help text will change because:
- `cooldown show` loses `--json` and gains `--format`
- `add`, `use`, `remove`, `refresh` gain `--format`

## Acceptance Criteria

- [x] `quota_styles` module is renamed to `styles` throughout `src/ui/mod.rs`
- [x] `GREEN` constant exists in the `styles` module
- [x] `account list` text output shows colored table with ▸ marker, ✓/✗ auth, human timestamps
- [x] `account current` shows `▸ {name} ({source})` (marker BOLD_CYAN, name BOLD, source DIM — per style guide SoT)
- [x] `account health` table has semantic colors (token, status, active, cooldown, fetched)
- [x] `account health --verbose` has semantic colors on values
- [x] `account cooldown show` table has colored status/countdown/headers
- [x] `account cooldown show --json` is replaced by `account cooldown show --format json`
- [x] `account add/use/remove/refresh` support `--format json`
- [x] `account add/use/remove/refresh` text output has colored ✓ prefix, name bold, path dim
- [x] `NO_COLOR=1 codex-session account list` produces no ANSI escape codes
- [x] `codex-session account list --format json | jq .` produces valid JSON
- [x] `just lint` passes (clippy-strict + fmt-check + print-ownership)
- [x] `just test` passes (with snapshot updates accepted)

## Next Round

Round 03 will apply the design system to every remaining output path: `doctor` report (colored
status table, summary), `config status` (colored key-value display), `config-recipe list/show/compose`
(colored tables), `version` (bold version numbers), error rendering (`error.rs` — colored labels
and status-aware hints), and runtime narration (`gate.rs` / `retry.rs` — colored warning prefixes,
styled `[codex-session]` narration).
