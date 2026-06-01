# Round 02: Beautiful UX Rendering + Full Test Suite

> Plan: doctor-refactor-completeness-ux | Round: 02 of 02 | Complexity: L (override) | Generated:
> 2026-05-27 | Repo: /workspaces/codex-session

## Context

The `codex-session doctor` subcommand was refactored in the previous round to use grouped checks
(`CheckGroup` model), add 6 new check functions (ping config, token probe, quota connectivity,
trust cache, session permissions, child version), and support `--online` for network checks. The
data model and check logic are complete, but the text rendering in `src/ui/mod.rs` still uses the
old plain-text format — uncolored `writeln!` calls with no visual hierarchy, no section grouping,
and no status symbols.

This round rewrites `write_doctor()` to produce beautiful, scannable output using the CLI design
system (shared styles module, color constants, status symbols) and the spinner module (for online
check progress). The JSON output serialization was updated in Round 01 to match the grouped
structure; this round focuses entirely on text rendering and test completeness.

## Previous Rounds

Round 01 produced:

- `--online` flag in `DoctorArgs`
- `CheckGroup` struct with `name: String` and `checks: Vec<CheckResult>`
- `DoctorReport` restructured with `groups: Vec<CheckGroup>`
- 6 new checks: `config-recipe.ping-profile`, `online.token-probe`, `online.quota-api`,
  `session.trust-cache`, `session.permissions`, `environment.child-version`
- Existing checks assigned to groups: Environment, Accounts, Config Recipe, Session, Auth, Online
- Updated JSON serialization, `hint_for()`, `populate_next_steps()`
- Basic test updates for compilation

## Scope of This Round

**IN scope:**

- Rewrite `write_doctor()` text rendering in `src/ui/mod.rs` with:
  - Section headers per `CheckGroup` (colored, bold)
  - Status symbols: ✓ (green) for OK, ⚠ (yellow) for WARN, ✗ (red) for FAIL
  - Aligned check table within each section
  - Styled account overview section (replace raw key=value with formatted table)
  - Summary banner with color (green/yellow/red based on worst status)
  - Env dump section with styling
  - Next-steps section with styling
- Spinner integration: show per-check spinner for online checks in TTY text mode
- Full test suite updates for new output format
- Ensure non-TTY / piped output is clean (no ANSI, no spinner artifacts)
- Ensure `--format json` is unaffected by rendering changes

**OUT of scope:**

- New check logic (done in Round 01)
- Changes to `DoctorReport` struct fields (done in Round 01)
- Changes to check grouping (done in Round 01)

## Current State

### Key Files

- `/workspaces/codex-session/src/ui/mod.rs` — The `write_doctor()` method (starting around line
  240) currently renders the report with plain `writeln!` calls. After the design system plan, this
  file has a shared styles module (renamed from `quota_styles` to `styles` or similar) with color
  constants like `BOLD`, `DIM`, `BOLD_CYAN`, `RED`, `BOLD_GREEN`, `BOLD_YELLOW`, `BOLD_RED`, and
  helper functions `style_open()`, `style_close()`. The `account quota` rendering
  (`write_quota_entry_text()`, around line 809) is the gold standard for how styled output works.

  Current `write_doctor()` structure (to be replaced):

  ```rust
  pub(crate) fn write_doctor(&self, report: &DoctorReport, fmt: OutputFormat) -> std::io::Result<()> {
      let mut stdout = std::io::stdout().lock();
      match fmt {
          OutputFormat::Text => {
              writeln!(stdout, "account:         {}", report.account)?;
              writeln!(stdout, "account-source:  {}", report.account_source)?;
              // ... flat key=value pairs ...
              // ... flat check table ...
              // ... summary line ...
          }
          OutputFormat::Json => write_json_line(&mut stdout, report),
      }
  }
  ```

- `/workspaces/codex-session/src/ui/color.rs` — Color policy: `should_color(Stream::Stdout)`
  returns `bool`. Respects `NO_COLOR`, `FORCE_COLOR`, `CLICOLOR_FORCE`, `CLICOLOR`, and TTY
  detection. All color usage must go through `should_color()` → `style_open()`/`style_close()`.

- `/workspaces/codex-session/src/ui/spinner.rs` — Spinner module (created by the spinner plan).
  Provides reusable spinner helpers built on `indicatif::MultiProgress`. Spinners auto-suppress
  in non-TTY and `--format json` mode.

- `/workspaces/codex-session/src/commands/doctor.rs` — After Round 01, `DoctorReport` has
  `groups: Vec<CheckGroup>` and the `CheckGroup` struct. The `build_report()` function collects
  checks into groups. The `run()` function calls `ctx.ui.write_doctor(&report, fmt)`.

- `/workspaces/codex-session/tests/cmd_doctor.rs` — Integration tests. After Round 01, tests
  compile but many string assertions may still reference old output format. This round updates
  them to match the new styled output.

### Existing Patterns

- **Styled output pattern** (from `write_quota_entry_text()`):

  ```rust
  let c = color::should_color(color::Stream::Stdout);
  write!(w, "{}", style_open(styles::BOLD_CYAN, c))?;
  write!(w, "Account: {}", name)?;
  write!(w, "{}", style_close(styles::BOLD_CYAN, c))?;
  ```

- **Section separator pattern** (use consistent visual breaks between groups):

  ```text
  ── Environment ──────────────────────────────
    ✓ xdg.paths          XDG_CONFIG_HOME=/home/user/.config ...
    ✓ child.binary        /usr/bin/codex (codex 1.2.3)
    ✓ child-version       codex 1.2.3 — compatible

  ── Accounts ─────────────────────────────────
    ✓ account.active.auth active account 'work' has auth
    ⚠ account.cooldowns   1 account(s) in cooldown: personal
  ```

- **Summary banner pattern** (color-coded based on worst status):

  ```text
  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ 12 OK  ⚠ 2 WARN  ✗ 0 FAIL
  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ```

- **Account overview** (replace raw key=value with table):

  ```text
  Account:    work (source: lru)
  Group ID:   pts-0 (source: tty)
  CODEX_HOME: /run/user/1000/codex-session/accounts/work/groups/pts-0
  Recipe:     default
  ```

## Implementation Steps

### Step 1: Define section rendering helpers

In `/workspaces/codex-session/src/ui/mod.rs`, add helper functions for the doctor rendering. These
are private to the module (not public API):

1. `write_doctor_header()` — Renders the account overview block (Account, Group ID, CODEX_HOME,
   Recipe) with styled labels. Uses `BOLD` for labels, `DIM` for source annotations.

2. `write_doctor_accounts_table()` — Renders the accounts list as a compact table with columns:
   name, auth status (✓/✗), cooldown status, last used (human-readable time). Replaces the raw
   `current=true has_auth=true last_used_at_unix=...` dump.

3. `write_doctor_group()` — Renders a single `CheckGroup` as a section with:
   - Section header line: `── <Group Name> ──────...` (BOLD, padded to terminal width or 60 chars)
   - Per-check rows: `<symbol> <check-name>  <detail>` with aligned columns
   - Status symbol: ✓ (green OK), ⚠ (yellow WARN), ✗ (red FAIL)

4. `write_doctor_summary()` — Renders the summary banner:
   - Horizontal rule (━━━)
   - Summary counts with colored symbols
   - Overall verdict color: green if 0 fail + 0 warn, yellow if warns but no fail, red if any fail

5. `write_doctor_next_steps()` — Renders next-steps with bullet points and styled check names.

6. `write_doctor_env()` — Renders the env dump sections with styled config-recipe names and
   key=value pairs (secrets show `***` with DIM styling).

### Step 2: Rewrite write_doctor text branch

Replace the entire `OutputFormat::Text` branch of `write_doctor()`. The new structure:

```text
1. Account overview header (write_doctor_header)
2. Accounts table (write_doctor_accounts_table)
3. Blank line separator
4. For each group in report.groups:
    a. Section header (write_doctor_group)
    b. Check rows within group
5. Summary banner (write_doctor_summary)
6. Next-steps section if non-empty (write_doctor_next_steps)
7. Env dump if non-empty (write_doctor_env)
```

The JSON branch (`OutputFormat::Json`) remains unchanged — it delegates to `write_json_line()`.

All style application uses the existing pattern:

```rust
let c = color::should_color(color::Stream::Stdout);
```

Passed to all helper functions as a `bool`. When `c` is false (piped, `NO_COLOR`), the helpers
produce clean text with no ANSI escapes — symbols still appear (UTF-8) but without color.

### Step 3: Add spinner integration for online checks

When `--online` is active and text output is going to a TTY, integrate the spinner module to show
progress during network checks. The spinner should:

- Start before the online check group runs (in `build_report()` or `run()`)
- Show a message like `"Checking token validity..."` / `"Checking quota API..."`
- Finish with ✓ or ✗ before rendering the check result

This requires modifying `/workspaces/codex-session/src/commands/doctor.rs` `run()` function to:

1. Build the non-online portion of the report first
2. If `--online` and TTY and text format: start a spinner, run online checks, stop spinner
3. Then render the complete report

The exact spinner API depends on what the spinner plan produced in `src/ui/spinner.rs`. Adapt to
the available API. If the spinner module provides a `SpinnerGroup` or `MultiProgress` wrapper, use
it. If it provides simpler single-spinner helpers, use those.

In non-TTY or JSON mode, spinners are automatically suppressed (per the spinner plan's design).

### Step 4: Handle format_status symbol rendering

Replace the existing `format_status()` function (which likely returns `"OK"`, `"WARN"`, `"FAIL"`
strings) with a styled version:

```rust
fn format_status_symbol(status: CheckStatus, use_color: bool) -> String {
    match status {
        CheckStatus::Ok => format!(
            "{}✓{}",
            style_open(styles::BOLD_GREEN, use_color),
            style_close(styles::BOLD_GREEN, use_color),
        ),
        CheckStatus::Warn => format!(
            "{}⚠{}",
            style_open(styles::BOLD_YELLOW, use_color),
            style_close(styles::BOLD_YELLOW, use_color),
        ),
        CheckStatus::Fail => format!(
            "{}✗{}",
            style_open(styles::BOLD_RED, use_color),
            style_close(styles::BOLD_RED, use_color),
        ),
    }
}
```

Keep the old `format_status()` if it's used elsewhere, or replace it entirely if it's only used
in `write_doctor()`.

### Step 5: Update integration tests for new text output

In `/workspaces/codex-session/tests/cmd_doctor.rs`, update all tests that assert on stdout content:

- `doctor_happy_path_exits_zero`: Update to check for section headers and status symbols. Replace
  `predicate::str::contains("OK")` with `predicate::str::contains("✓")`. Verify group headers
  appear (e.g., `contains("Environment")`).

- `doctor_json_shape`: Update JSON navigation to use grouped structure. Replace
  `value["checks"].as_array()` with navigating `value["groups"]` to find checks within groups.

- `doctor_fails_when_active_account_missing_auth`: Update FAIL assertion to use `✗` symbol.

- `doctor_warn_cooldown_appears_in_next_steps`: Update WARN assertion to use `⚠` symbol.

- `doctor_warns_when_stock_mode`: Check for section header and stock mode indicator.

- All other tests: Update string assertions to match new output format while preserving the
  semantic checks (correct check names, correct status, correct detail content).

For piped output (which all `assert_cmd` tests produce), verify that no ANSI escapes appear.
The tests run with stdout piped to a buffer, so `should_color(Stream::Stdout)` returns false.
Symbols (✓/⚠/✗) are UTF-8 and appear in piped output, but without color codes.

### Step 6: Add rendering-specific tests

Add new tests:

- `doctor_text_output_has_section_headers`: Verify each group name appears as a section header.
- `doctor_text_output_has_summary_banner`: Verify the summary line format with ✓/⚠/✗ counts.
- `doctor_text_output_accounts_table`: Verify the account overview is formatted as a table (not
  raw `current=true has_auth=true`).
- `doctor_json_output_unaffected_by_rendering`: Verify JSON output has no ANSI escapes and
  matches the expected schema shape.
- `doctor_piped_output_no_ansi`: Explicitly verify no `\x1b[` sequences in piped stdout.

### Final Step: Update plan index and move to done

Update the plan's `README.md` (in the same directory as this round file) to record completion:

1. In the `## Execution Order` table, find the row for round 02.
2. Change `Status` from `todo` to `done`.
3. Change `Completed` from `--` to today's date (`YYYY-MM-DD`).
4. In the README.md header blockquote, change `Status: todo` to `Status: done`.
5. Move the plan directory to done:

```bash
mkdir -p .plan/02-done && mv .plan/01-todo/doctor-refactor-completeness-ux .plan/02-done/doctor-refactor-completeness-ux
```

## Acceptance Criteria

- [ ] `write_doctor()` text branch completely rewritten with styled output
- [ ] Section headers appear for each `CheckGroup` (colored bold in TTY)
- [ ] Status symbols: ✓ (green), ⚠ (yellow), ✗ (red) render correctly
- [ ] Account overview block replaces raw key=value dump
- [ ] Accounts table shows name, auth, cooldown, last-used in human-readable format
- [ ] Summary banner with color-coded counts
- [ ] Next-steps section styled with bullet points
- [ ] Env dump section styled with config-recipe headers
- [ ] Spinners show during `--online` checks in TTY text mode
- [ ] Spinners suppressed in non-TTY, `--format json`, and `NO_COLOR` modes
- [ ] No ANSI escapes in piped output
- [ ] JSON output unchanged from Round 01
- [ ] All existing tests updated and passing
- [ ] New rendering-specific tests added
- [ ] `just test` passes
- [ ] `just lint` passes
- [ ] Plan `README.md` execution order table shows round 02 as `done` with today's date
- [ ] Plan `README.md` header status is `done`
- [ ] Plan directory moved from `.plan/01-todo/doctor-refactor-completeness-ux` to `.plan/02-done/doctor-refactor-completeness-ux`

## Next Round

This is the final round.
