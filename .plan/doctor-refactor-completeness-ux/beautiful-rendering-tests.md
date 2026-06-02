# Round 02: Beautiful UX Rendering + Full Test Suite

> Plan: 04-doctor-refactor-completeness-ux | Round: 02 of 02 | Complexity: L (override) |
> Executor: prex (EF 1.5) | Generated: 2026-05-27 | Updated: 2026-06-02 | Repo: /workspaces/codex-session

## Context

Round 01 refactored `doctor` to use a grouped report model (`CheckGroup`), added the new checks
(ping config, concurrent token-probe + quota-connectivity under `--online`, trust cache, session
permissions), assigned every existing check to a group, and updated the JSON serialization. The
data model is complete, but the **text** branch of `write_doctor` still iterates the (now removed)
flat check list and renders text status labels (`OK`/`WARN`/`FAIL`) instead of ✓/⚠/✗.

Crucially, the dependency plans already delivered most of the styling: `write_doctor` already uses
the `styles` module, colors, a colored summary banner, a styled account list (✓/✗ for `has_auth`),
and the spinner is already wired into `doctor::run()`. This round closes the remaining gap —
iterate groups with section headers, use ✓/⚠/✗ symbols in check rows, render human-readable
timestamps, set online-aware spinner messages, and finish the test updates. **No struct, grouping,
or check-logic changes** (those were Round 01).

## Previous Rounds

Round 01 produced:

- `--online` flag in `DoctorArgs` (still `Copy`).
- `CheckGroup { name: String, checks: Vec<CheckResult> }`; `DoctorReport` now has
  `groups: Vec<CheckGroup>` plus `DoctorReport::all_checks()`.
- New checks: `config-recipe.ping-profile`, `online.token-probe`, `online.quota-api`,
  `session.trust-cache`, `session.permissions`.
- Every existing check assigned to a group: Environment, Accounts, Config Recipe, Session, Auth,
  Online (Online only with `--online` + resolved account).
- Concurrent online runner (`block_on(tokio::join!(...))`); updated `hint_for`/`is_actionable_warn`.
- JSON serialization updated; `run`/`build_report` remain sync; text rendering still flat.

## Scope of This Round

**IN scope:**

- Rewrite the `OutputFormat::Text` branch of `write_doctor` in `src/ui/mod.rs` to iterate
  `report.groups` with a section header per group and aligned check rows.
- Replace text status labels in check rows with ✓ (green) / ⚠ (yellow) / ✗ (red) symbols.
- Render `last_used_at_unix` and cooldown reset times as human-readable durations.
- Set online-aware spinner messages (reuse the existing single spinner; do NOT restructure `run`).
- Update/extend integration tests for the new text output; verify piped output has no ANSI.
- Keep `--format json` output byte-identical to Round 01 (rendering changes are text-only).

**OUT of scope (done in Round 01):**

- New check logic, `DoctorReport`/`CheckGroup` field changes, grouping membership.

## Current State

### `src/ui/mod.rs`

`write_doctor` (the `OutputFormat::Text` branch) currently renders a header block, a styled account
list, then a **flat** check table built from `report.checks` (now `report.groups`), followed by
next-steps, env dump, and a colored summary banner. It already computes
`let use_color = color::should_color(color::Stream::Stdout);` and uses the helpers below. The
JSON branch delegates to `write_json_line(&mut stdout, report)` and must stay unchanged.

The styles module and helpers already exist (no rename needed):

```rust
mod styles {
    // BOLD, DIM, BOLD_CYAN, GREEN, RED, BOLD_GREEN, BOLD_YELLOW, BOLD_RED
}
pub(crate) fn style_open(style: anstyle::Style, use_color: bool) -> impl std::fmt::Display { /* .render() */ }
pub(crate) fn style_close(style: anstyle::Style, use_color: bool) -> impl std::fmt::Display { /* .render_reset() */ }
fn styled_text(text, style, use_color) -> String
fn styled_padded(text, width, style, use_color) -> String        // left-align
fn styled_padded_right(text, width, style, use_color) -> String  // right-align
```

The status formatter currently returns text labels:

```rust
const fn format_status(status: crate::commands::doctor::CheckStatus) -> &'static str {
    match status {
        CheckStatus::Ok => "OK",
        CheckStatus::Warn => "WARN",
        CheckStatus::Fail => "FAIL",
    }
}
```

A human-time helper already exists for quota rendering, e.g. `human_duration_until(reset_at_unix)`
(used in `write_quota_*`). Reuse it; add a "since/ago" variant if `last_used` needs elapsed time.

### `src/commands/doctor.rs`

`build_report` already runs the online checks inside itself with the shared spinner via
`set_progress(progress, "...")`. `run` builds the `SpinnerGroup`, finishes it, then calls
`ctx.ui.write_doctor(&report, fmt)`. Group display names map from the `GROUP_*` constants
(`"environment"` → `"Environment"`, etc.).

### `src/ui/spinner.rs` / `color.rs`

`SpinnerGroup::new(visible)` + `handle.set_message/finish_ok/finish_err`; visibility is
pre-computed by `should_show_spinner` (suppresses in non-TTY and JSON). `should_color(Stream)`
gates all color. In piped output `use_color` is false, so symbols (UTF-8) print without ANSI.

## Implementation Steps

### Step 1: Group-section helper + title map

In `src/ui/mod.rs` add private helpers:

- A group-name → display-title map (`"environment"` → `"Environment"`, `"accounts"` → `"Accounts"`,
  `"config-recipe"` → `"Config Recipe"`, `"session"` → `"Session"`, `"auth"` → `"Auth"`,
  `"online"` → `"Online"`), falling back to the raw name.
- `write_doctor_group(stdout, group, use_color)` — renders a header line
  `── <Title> ──────…` (BOLD, padded to ~60 chars) then one row per check:
  `<symbol>  <padded check name>  <detail>`, reusing `styled_padded` for the name column
  (width = max check-name length within the group, min 8).

### Step 2: Status symbol formatter

Add a symbol formatter (keep `format_status` if still referenced elsewhere):

```rust
fn format_status_symbol(status: crate::commands::doctor::CheckStatus, use_color: bool) -> String {
    let (sym, style) = match status {
        CheckStatus::Ok   => ("✓", styles::BOLD_GREEN),
        CheckStatus::Warn => ("⚠", styles::BOLD_YELLOW),
        CheckStatus::Fail => ("✗", styles::BOLD_RED),
    };
    format!("{}{sym}{}", style_open(style, use_color), style_close(style, use_color))
}
```

### Step 3: Rewrite the text branch to iterate groups

Replace the flat `for check in &report.checks { ... }` table with:

```text
1. Account-overview header (keep existing styled lines; lightly restyle leading key=value rows
   into BOLD-labelled rows for consistency).
2. Styled account list (existing) — render last-used via the human-time helper (Step 4).
3. Blank line.
4. for group in &report.groups:
       write_doctor_group(&mut stdout, group, use_color)   // header + check rows
       blank line between groups
5. Summary banner (existing colored counts; optionally bracket with ━━━ rules).
6. Next-steps section if non-empty (existing).
7. Env dump if non-empty (existing).
```

The Online group only exists in `report.groups` when `--online` was passed (Round 01), so the
Online section header appears only then — no extra conditionals needed here.

### Step 4: Human-readable timestamps

In the account list, replace the raw `last_used_at_unix` integer with a human-readable rendering
(e.g. "3h ago" / "just now" / "(never)") using the existing time helper (add an elapsed-since
variant if only `human_duration_until` exists). Render cooldown reset times the same way.

### Step 5: Online-aware spinner messages (no `run` restructure)

The online checks already run inside `build_report` with the shared spinner. Confirm the message set
in Round 01 (`"Probing token + quota (parallel)..."`) reads well, and add a post-join
`set_progress(progress, "Online checks complete")` if helpful. Do **not** add a per-check spinner
system or make `run` async. Non-TTY/JSON suppression is already handled by `should_show_spinner`.

### Step 6: Tests

In `tests/cmd_doctor.rs` (+ `account_doctor.rs`), update text-output assertions:

- Swap status-label assertions for symbols where check rows are asserted: `contains("✓")`,
  `contains("⚠")`, `contains("✗")` (the summary banner may keep its `N OK / N WARN / N FAIL` text —
  match the actual rendering chosen).
- Verify section headers render: `contains("Environment")`, `contains("Config Recipe")`,
  `contains("Session")`, etc.
- `doctor_online_flag_runs_network_checks`: verify the `Online` section header appears with
  `--online`; `doctor_default_omits_online_group`: verify it does not without it.
- Add `doctor_piped_output_no_ansi`: assert no `\x1b[` sequences in piped stdout.
- Add `doctor_text_output_has_section_headers` and `doctor_text_output_has_summary_banner`.
- Confirm `--format json` output is unchanged from Round 01 (re-run the JSON-shape test).

### Final Step: Update plan index and move to done

1. In this directory's `README.md` Execution Order table, set round 02 `Status` → `done`,
   `Completed` → today's date.
2. In the `README.md` header blockquote, change `Status: todo` → `Status: done`.
3. Move the plan directory:

```bash
mkdir -p .plan/02-done && mv .plan/01-todo/04-doctor-refactor-completeness-ux .plan/02-done/04-doctor-refactor-completeness-ux
```

## Acceptance Criteria

- [ ] The `OutputFormat::Text` branch iterates `report.groups`; each group renders a section header.
- [ ] Check rows use ✓ (green) / ⚠ (yellow) / ✗ (red) symbols via `format_status_symbol`.
- [ ] `last_used` and cooldown reset render as human-readable durations (no raw unix integers).
- [ ] Online-aware spinner message is set; `run`/`build_report` remain sync (no restructure).
- [ ] No ANSI escapes in piped output; symbols still print (UTF-8).
- [ ] `--format json` output is byte-identical to Round 01.
- [ ] Existing text tests updated to symbols/headers; new rendering tests added.
- [ ] `just test` and `just lint` pass.
- [ ] README round 02 row is `done` with today's date; header `Status: done`.
- [ ] Plan directory moved to `.plan/02-done/04-doctor-refactor-completeness-ux`.

## Next Round

This is the final round.
