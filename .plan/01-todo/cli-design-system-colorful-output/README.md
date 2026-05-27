# CLI Design System & Colorful, Consistent Output

> Complexity: L | Rounds: 3 | Generated: 2026-05-26 | Repo: /workspaces/codex-session | Status: todo

## Problem Statement

codex-session's CLI output is inconsistent across commands. `account quota` has colorful bars,
human-readable timestamps, and conditional ANSI styling — it's the gold standard. Meanwhile
`account list` is a raw key=value dump, `doctor` and `config status` are plain-text tables with no
color, `health` and `cooldown` have tables but no color, and error rendering uses ad-hoc bold
labels. There is no shared design language documenting what colors mean, how tables should look, or
how user-facing output differs from structured logs.

Additionally, the project has two distinct output channels with different goals that need formal
separation:

- **User-facing output** (stdout for commands, stderr for warnings/prompts): should be beautiful,
  scannable, and consistent — colored tables, status indicators, human-readable times.
- **Structured logs** (file + optional stderr mirror): should be machine-parseable, AI-friendly,
  and detail-rich — JSON with structured fields for debugging and observability.

This plan creates a CLI Design System specification, then systematically applies it to every
wrapper-owned command and error path.

## Strategy

The work splits into three rounds following layer boundaries (bottom-up):

1. **Round 01 — Design System Spec**: Write the authoritative reference document in `docs/design/`.
  No code changes. This document defines every color, every layout pattern, every symbol, and the
  stdout/stderr/log boundary rules. All subsequent rounds reference it.

2. **Round 02 — Foundations + Account Commands**: Rename `quota_styles` → `styles`, add missing
  color constants, then apply the design system to all account commands (list, current, health,
  cooldown show, mutations). Migrate remaining `--json` flags to `--format json`. The account
  quota command is already styled — skip it.

3. **Round 03 — Doctor, Config, ConfigRecipe, Version, Errors, Narration**: Apply the design system to
  every remaining output path: `doctor` report, `config status`, `config_recipe list/show/compose`,
  `version`, error rendering (`error.rs`), and runtime narration (`gate.rs` / `retry.rs`
  `write_warning`/`write_prompt`).

## Execution Order

| Round | File                                    | Topic                             | Status | Completed |
| ----- | --------------------------------------- | --------------------------------- | ------ | --------- |
| 01    | `01-design-system-spec.md`              | CLI design system specification   | todo   | --        |
| 02    | `02-foundations-and-account-commands.md` | Styles module + account commands  | todo   | --        |
| 03    | `03-doctor-config-config_recipe-errors.md`    | Doctor, config, config_recipe, errors   | todo   | --        |

## Execution Commands

```bash
# Execute a single round:
/plex .plan/01-todo/cli-design-system-colorful-output/01-design-system-spec.md
/plex .plan/01-todo/cli-design-system-colorful-output/02-foundations-and-account-commands.md
/plex .plan/01-todo/cli-design-system-colorful-output/03-doctor-config-config_recipe-errors.md

# Execute with full directory context:
/prex -ar @.plan/01-todo/cli-design-system-colorful-output/
```

## Decisions & Constraints

1. **Pre-v1.0 breaking changes are acceptable.** codex-session has not committed to a stable
  interface. Make clean breaks — no hidden aliases, deprecation shims, or compatibility layers.

2. **Use `--format json` everywhere, never `--json`.** Upstream `codex` already uses `--json` for
  JSONL event streaming. Our wrapper uses `--format json` (via `OutputFormat` enum) to avoid flag
  collision when the user passes flags through to the child.

3. **Design system spec goes in `docs/design/`.** It can span multiple files (e.g., a main spec
  plus a color reference). It is committed to git and referenced from `CLAUDE.md`.

4. **Two message channels: user-facing vs logs.** User-facing output (stdout commands + stderr
  warnings/prompts/errors) gets the full design system treatment. Structured logs (`tracing::*`
  macros → file/stderr mirror) stay machine-readable JSON — no ANSI, no tables.

5. **No new dependencies.** Everything is built on the existing `anstyle` crate, `color::should_color()`,
  `style_open()`/`style_close()`, `human_age()`, `human_duration_until()`.

6. **`account quota` is the gold standard.** Its patterns (colored bars, human times, conditional
  ANSI, rank/score display) are extended to other commands, not modified.

7. **`NO_COLOR` / `FORCE_COLOR` / `CLICOLOR` respected everywhere.** The existing `color.rs`
  module handles this. All new color usage must call `color::should_color()` and pass the result
  through the `style_open()`/`style_close()` gate.

## Rejected Alternatives

- **Using a TUI framework (ratatui, tui-rs):** Over-engineered for a CLI wrapper. The commands
  produce one-shot output, not interactive screens.

- **Adding `colored` or `owo-colors` crate:** Would duplicate functionality already provided by
  `anstyle` + the existing style infrastructure.

- **Keeping `--json` on cooldown show:** Would create an inconsistency with the rest of the wrapper
  and risk colliding with upstream codex's `--json` flag.

## Risks & Edge Cases

- **Snapshot test churn:** Every command's help text will change when `--format` is added. Plan
  includes snapshot updates. Accepted risk — tests verify the new state.

- **Color in piped output:** `color::should_color()` already returns false for non-TTY. All new
  color code goes through the existing gate. Low risk.

- **Wide Unicode in tables (▸, ✓, ✗):** Terminal column width for these characters is
  implementation-dependent. The spec should document fixed-width fallbacks or test on common
  terminals. Accepted risk — UTF-8 terminals are the primary target.

## Completion

When all rounds are done:

```bash
# Update status in this file to "done"
# Fill in completion timestamps in the execution order table
mv .plan/01-todo/cli-design-system-colorful-output .plan/02-done/cli-design-system-colorful-output
```
