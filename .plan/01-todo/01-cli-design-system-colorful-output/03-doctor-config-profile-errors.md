# Round 03: Doctor, Config, ConfigRecipe, Version, Errors & Narration

> Plan: cli-design-system-colorful-output | Round: 03 of 03 | Complexity: L | Generated: 2026-05-26
> Repo: /workspaces/codex-session

## Context

codex-session is a CLI wrapper around OpenAI's `codex` binary. Its user-facing output has been
inconsistent: `account quota` was the only command with full color treatment. A CLI Design System
specification now exists at `docs/design/cli-style-guide.md`, and all account commands have been
updated to follow it (Rounds 01 and 02).

This final round extends the design system to every remaining output path in the wrapper: the
`doctor` diagnostic report, `config status`, `config_recipe list/show/compose`, `version`, error
rendering, and runtime narration (warnings, prompts, and `[codex-session]` messages). After this
round, every user-visible string produced by the wrapper follows the same visual language.

## Previous Rounds

**Round 01** created:
- `docs/design/cli-style-guide.md` — CLI Design System specification
- Updated `CLAUDE.md` with design system reference and pre-v1.0 breaking changes policy

**Round 02** implemented:
- Renamed `quota_styles` → `styles` module in `src/ui/mod.rs`
- Added `GREEN` constant to the styles module
- Colorized `account list`, `account current`, `account health` (table + verbose),
  `account cooldown show`
- Colorized mutation output (add/use/remove/refresh) with `✓` prefix
- Migrated `cooldown show --json` → `--format json`
- Added `--format json` to add/use/remove/refresh commands
- All account commands now follow the design system

Expected state after Round 02:
- `styles::` module with constants: BOLD, DIM, BOLD_CYAN, GREEN, BOLD_GREEN, BOLD_YELLOW, RED,
  BOLD_RED
- Helper functions `style_open()`, `style_close()` unchanged
- All account commands produce colored text with `NO_COLOR` support

## Scope of This Round

**IN scope:**
- Colorize `write_doctor` output (check table with OK/WARN/FAIL colors, summary, accounts section)
- Colorize `write_config_status` output (key-value pairs with semantic colors)
- Colorize `write_config_recipe_list` output (table with ✓/✗ valid column)
- Colorize `write_config_recipe_show` output (key-value pairs)
- Colorize `write_config_recipe_compose` output (key-value pairs)
- Colorize `write_version` output (bold version numbers, account info)
- Enhance error rendering in `error.rs` — use design system colors for labels and status
- Colorize stderr narration: `write_warning()`, `write_prompt()`, gate.rs `narrate()`
- Update snapshot tests for any help text changes

**OUT of scope:**
- Account commands (already done in Round 02)
- Quota rendering (already the gold standard)
- Changes to the design system spec
- New dependencies
- Logging format changes (structured logs stay JSON, no ANSI)

## Current State

### Key Files

- `/workspaces/codex-session/src/ui/mod.rs` — Main UI module. After Round 02, the `styles` module
  exists with all color constants. Methods to modify in this round:
  - `write_doctor` (line 235) — plain text check table, accounts list, summary
  - `write_config_status` (line 84) — raw key=value dump
  - `write_config_recipe_list` (line 168) — plain text list
  - `write_config_recipe_show` (line 197) — plain text key-value
  - `write_config_recipe_compose` (line 508) — plain text key-value
  - `write_version` (line 59) — plain text
  - `write_warning` (line 48) — plain stderr
  - `write_prompt` (line 41) — plain stderr

- `/workspaces/codex-session/src/error.rs` — Error rendering module (933 lines). The `render()`
  function at line 175 produces structured error output. Currently uses `style_label()` (line 668)
  which does basic bold on labels. The error kind labels (`codex-session:`, `where:`, `why:`,
  `hint:`, `caused by:`) could use the design system colors.

- `/workspaces/codex-session/src/commands/doctor.rs` — Doctor command (1104 lines). Builds a
  `DoctorReport` with `CheckResult` entries (each has `name`, `status: CheckStatus`, `detail`).
  The `CheckStatus` enum has `Ok`, `Warn`, `Fail`. The `format_status()` function in `ui/mod.rs`
  maps these to strings but applies no color.

- `/workspaces/codex-session/src/services/account/gate.rs` — Account gate/resolver. The
  `narrate()` function at line 858 prints `[codex-session] {msg}` via `write_prompt()`.
  `warn_and_confirm_auth_missing()` at line 136 uses `write_warning()` for multi-line auth
  warnings.

- `/workspaces/codex-session/src/services/account/retry.rs` — Retry logic. Uses `write_warning()`
  at line 51 for the `--max-retries` warning.

- `/workspaces/codex-session/src/commands/account/quota.rs` — Uses `write_warning()` at line 18
  for deprecation notice.

- `/workspaces/codex-session/src/services/session/group_id.rs` — Uses `write_warning()` at
  line 106 for group-id resolution warnings.

### Existing Patterns

After Round 02, the established pattern for colorized output is:
```rust
let c = color::should_color(color::Stream::Stdout);
// For headers:
write!(stdout, "{}{}{}", style_open(styles::DIM, c), "HEADER", style_close(styles::DIM, c))?;
// For status values:
let status_style = match status {
    "ok" => styles::BOLD_GREEN,
    "warn" => styles::BOLD_YELLOW,
    "fail" => styles::BOLD_RED,
    _ => styles::BOLD,
};
write!(stdout, "{}{}{}", style_open(status_style, c), status, style_close(status_style, c))?;
```

For stderr, `color::stderr_color()` provides the boolean gate.

The `format_status()` function in `ui/mod.rs` converts `CheckStatus` to strings:
```rust
const fn format_status(status: CheckStatus) -> &'static str {
    match status {
        CheckStatus::Ok => "OK",
        CheckStatus::Warn => "WARN",
        CheckStatus::Fail => "FAIL",
    }
}
```

## Implementation Steps

### Step 1: Colorize `write_doctor`

**File:** `/workspaces/codex-session/src/ui/mod.rs` — method at line 235

This is the largest change in this round. The doctor output has multiple sections.

**Accounts section** (lines 253-279):
- Account name in BOLD
- `current=true` in BOLD_CYAN
- `has_auth=true` → GREEN ✓, `has_auth=false` → RED ✗
- `cooldown=active(...)` in BOLD_RED when active, plain otherwise

**Check table** (lines 282-304):
- Header row (`status`, `check`, `detail`) in DIM
- `OK` status in BOLD_GREEN
- `WARN` status in BOLD_YELLOW
- `FAIL` status in BOLD_RED
- Check names in BOLD

**Summary line** (lines 319-323):
- OK count in BOLD_GREEN
- WARN count in BOLD_YELLOW
- FAIL count in BOLD_RED

**Next steps** (lines 305-309):
- Each step as-is (the text is already actionable)

Add `let c = color::should_color(color::Stream::Stdout);` at the top of the text branch and
thread it through.

### Step 2: Colorize `write_config_status`

**File:** `/workspaces/codex-session/src/ui/mod.rs` — method at line 84

This is a key-value dump. Apply the design system:
- Labels (`active-config-recipe:`, `manifest-path:`, etc.) in DIM
- ConfigRecipe name value in BOLD when present
- `(stock mode)` / `(none)` / `(unavailable)` in DIM
- Account name in BOLD
- Path values in DIM
- Boolean `true`/`false` for `active-auth`: `true` → BOLD_GREEN, `false` → BOLD_RED
- Layer entries: name in BOLD, path in DIM, `exists=true` → GREEN, `exists=false` → RED
- Error sub-lines in BOLD_RED
- Log configuration values: plain (they're settings, not status)

### Step 3: Colorize `write_config_recipe_list`

**File:** `/workspaces/codex-session/src/ui/mod.rs` — method at line 168

Change from plain list to a colored table:

```text
CONFIG_RECIPE    LAYERS   VALID   MANIFEST
default    3        ✓       /path/to/default.yaml
work       2        ✗       /path/to/work.yaml
```

- Header in DIM
- ConfigRecipe name in BOLD
- Layer count: plain
- Valid: `✓` in GREEN if true, `✗` in RED if false
- Manifest path in DIM
- If error exists: show on indented line in BOLD_RED

Empty state `(no config recipes)` stays as-is.

### Step 4: Colorize `write_config_recipe_show`

**File:** `/workspaces/codex-session/src/ui/mod.rs` — method at line 197

- Labels (`config-recipe:`, `manifest:`, `layers:`) in DIM
- ConfigRecipe name in BOLD
- Path values in DIM
- `stock mode` text in DIM
- Layer entries: name in BOLD, path in DIM, `exists=true` → GREEN, `exists=false` → RED

### Step 5: Colorize `write_config_recipe_compose`

**File:** `/workspaces/codex-session/src/ui/mod.rs` — method at line 508

- Labels (`config-recipe:`, `group-id:`, `session-dir:`, `config:`, `sidecar:`, `session-meta:`) in DIM
- ConfigRecipe name in BOLD
- Group-id value in BOLD
- Path values in DIM

### Step 6: Colorize `write_version`

**File:** `/workspaces/codex-session/src/ui/mod.rs` — method at line 59

- `codex-session` text: plain
- Version number in BOLD
- `codex` path in DIM
- Child version in BOLD
- `(unknown)` / `(unresolved)` in BOLD_YELLOW
- Account line: account name in BOLD, source in DIM

### Step 7: Enhance error rendering

**File:** `/workspaces/codex-session/src/error.rs`

The current `render()` function (line 175) uses `style_label()` for bold on label names. Enhance:

- `codex-session:` label: BOLD_RED (it's an error, signal severity)
- `where:` label: BOLD
- `why:` label: BOLD
- `hint:` label: BOLD_CYAN (hints are actionable, highlight them)
- `caused by:` label: BOLD (or DIM — subordinate to the main error)
- The `what` message after `codex-session:`: plain (the label provides the color)

Update `style_label()` to accept a style parameter (or create `style_error_label()` that takes a
style and the use_color boolean). The `use_color` is already computed from `stderr_color()`.

### Step 8: Colorize stderr narration

**File:** `/workspaces/codex-session/src/ui/mod.rs`

**`write_warning`** — Add optional color support:
- When the body starts with `"warning:"`, style that prefix in BOLD_YELLOW
- The rest of the message stays plain
- Gate on `color::stderr_color()`

**`write_prompt`** — Leave mostly plain (prompts are interactive, color adds noise to input lines).
No change needed.

**File:** `/workspaces/codex-session/src/services/account/gate.rs`

**`narrate()`** function at line 858:
- Style the `[codex-session]` prefix in DIM
- The message text stays plain
- Gate on `color::stderr_color()`

This requires either:
- Passing `use_color` from `gate.rs` (it has access to `ctx.ui`)
- Or having `write_prompt` accept an optional style hint

The simplest approach: modify `narrate()` to call `color::stderr_color()` directly and build the
styled prefix before passing to `write_prompt()`. The `write_prompt()` method writes raw bytes, so
the ANSI codes can be embedded in the string.

### Step 9: Update snapshot tests

Run `just test` and update any snapshots that changed due to help text modifications.

If any new `--format` flags were added to non-account commands in this round (unlikely — they
already have global `--format` or per-command format), update the corresponding snapshots.

## Acceptance Criteria

- [ ] `doctor` output has colored check statuses (OK green, WARN yellow, FAIL red)
- [ ] `doctor` summary line has colored counts
- [ ] `config status` has DIM labels and colored boolean/status values
- [ ] `config_recipe list` shows a colored table with ✓/✗ valid column
- [ ] `config_recipe show` has DIM labels and BOLD values
- [ ] `config_recipe compose` has DIM labels and appropriate value colors
- [ ] `version` has BOLD version numbers and DIM paths
- [ ] Error output uses BOLD_RED for the `codex-session:` label and BOLD_CYAN for `hint:`
- [ ] `write_warning` styles `warning:` prefix in BOLD_YELLOW on stderr
- [ ] `[codex-session]` narration prefix is DIM on stderr
- [ ] `NO_COLOR=1 codex-session doctor` produces no ANSI escape codes
- [ ] All JSON output paths remain unaffected (no ANSI in JSON)
- [ ] `just lint` passes
- [ ] `just test` passes (with snapshot updates accepted)
- [ ] Manual verification: run each command and visually confirm consistency with the design system

## Next Round

This is the final round.
