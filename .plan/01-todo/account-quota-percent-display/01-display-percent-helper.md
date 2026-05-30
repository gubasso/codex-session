# Round 01: Display helper + renderer + tests

> Plan: account-quota-percent-display | Round: 01 of 01 | Complexity: S Generated: 2026-05-30 |
> Repo: /workspaces/codex-session

## Context

`codex-session account quota` prints a window's remaining quota like `99.0% left` even when the
window has just reset and is effectively unused. Users expect `100% left` in that case.

The ChatGPT backend endpoint `GET https://chatgpt.com/backend-api/wham/usage` reports usage as a
coarse **integer** `used_percent` per window, and that is the *only* usage signal (no token counts,
no `percent_left` field). A near-empty window returns `used_percent: 1`, which the wrapper
faithfully converts to `percent_left = 100 - used_percent = 99.0` and renders as `99.0%`. The
arithmetic is correct; the issue is purely presentation.

Fix: keep parsing faithful and change only the human-facing text rendering. Display whole numbers
and force the near-empty top bucket (`used_percent <= 1`, equivalently `percent_left >= 99`) to read
`100%`. Mid-range values are unchanged. Examples: `used_percent 0 -> 100%`, `1 -> 100%`,
`13 -> 87%`, `35 -> 65%`, `100 -> 0%`.

## Previous Rounds

This is the first round — no prior rounds.

## Scope of This Round

IN scope:

- A small pure helper in `src/ui/mod.rs` mapping faithful `percent_left` to a whole-number display
  value (`u32`), with the top-bucket special case and `0..=100` clamping.
- Updating the two text-rendering sites in `write_quota_entry_text` (five-hour and weekly) to use
  the helper for BOTH the printed number and the progress bar.
- Unit test(s) for the helper.

OUT of scope:

- Any change to `src/services/account/quota.rs` parsing (`parse_window`, `Window.percent_left`).
- Verbose-mode output (`write_quota_entry_verbose`) — keep it faithful (`{:.1}`).
- JSON output (`--format json`) — keep it faithful (serializes raw `percent_left`).
- Colors, layout, table structure, stdout/stderr ownership.

## Current State

### Key Files

- `/workspaces/codex-session/src/ui/mod.rs` — owns quota text rendering. Relevant functions:

  `quota_bar` (computes the filled/empty block bar from a percentage):

  ```rust
  fn quota_bar(pct: f64, width: usize, use_color: bool) -> String {
      #[allow(
          clippy::cast_possible_truncation,
          clippy::cast_sign_loss,
          clippy::cast_precision_loss
      )]
      let filled = (pct / 100.0 * width as f64).round() as usize;
      let filled = filled.min(width);
      let empty = width - filled;
      let style = percent_style(pct);
      format!(
          "{}{}{}{}",
          style_open(style, use_color),
          "█".repeat(filled),
          style_close(style, use_color),
          "░".repeat(empty),
      )
  }
  ```

  `write_quota_entry_text` — the two `oauth` rendering sites that currently print `{:.1}%` of the
  raw `percent_left`:

  ```rust
  if let Some(ref fh) = view.five_hour {
      let ps = percent_style(fh.percent_left);
      writeln!(
          stdout,
          "  Five-hour   {}  {}{:.1}%{} left   resets in {}",
          quota_bar(fh.percent_left, 20, use_color),
          style_open(ps, use_color),
          fh.percent_left,
          style_close(ps, use_color),
          human_duration_until(fh.reset_at_unix),
      )?;
  }
  if let Some(ref wk) = view.weekly {
      let ps = percent_style(wk.percent_left);
      writeln!(
          stdout,
          "  Weekly      {}  {}{:.1}%{} left   resets in {}",
          quota_bar(wk.percent_left, 20, use_color),
          style_open(ps, use_color),
          wk.percent_left,
          style_close(ps, use_color),
          human_duration_until(wk.reset_at_unix),
      )?;
  }
  ```

  `view.five_hour` / `view.weekly` are `Option`s on
  `crate::commands::account::AccountQuotaEntryView`; each carries a `percent_left: f64` and a
  `reset_at_unix`.

- `/workspaces/codex-session/src/services/account/quota.rs` — NOT modified this round.
  `parse_window` sets `percent_left = 100.0 - used_percent` and that semantics stays. (Context only.)

- `/workspaces/codex-session/src/ui/mod.rs` `write_quota_entry_verbose` — prints
  `five_hour_pct: {:.1}` / `weekly_pct: {:.1}` of raw `percent_left`. Leave UNCHANGED.

- `/workspaces/codex-session/tests/account_quota_cli.rs` — integration test; its mock feeds
  `percent_left` directly via `payload(five_hour, weekly)` and asserts presence of `"Five-hour"` /
  `"Weekly"` (not exact percentages), so the format change does not break it. Useful place to add an
  assertion that a near-full window prints `100%`.

### Existing Patterns

- Renderer color is chosen by `percent_style(pct: f64)`; the bar uses `quota_bar(pct, 20, ...)`.
  Both currently take the raw `percent_left`. Keep passing a `f64` to them, but pass the
  display-adjusted value so number, color, and bar all agree.
- `docs/design/cli-style-guide.md` governs this renderer (per `CLAUDE.md`). This change only alters
  the numeric token from `{:.1}%` to a whole-number `{}%`; preserve spacing, the `left` / `resets
  in` wording, colors, and stdout ownership exactly.
- Tests in this repo use `cargo-nextest` via `just`; prefer `just test-unit` over raw cargo.

## Implementation Steps

### Step 1: Add the `display_percent_left` helper

In `/workspaces/codex-session/src/ui/mod.rs`, add a pure helper near `quota_bar` / `percent_style`:

```rust
/// Map the faithful `percent_left` (== `100 - used_percent`, where the backend's
/// `used_percent` is a coarse integer) to the whole-number value shown to the user.
///
/// The backend reports a near-empty window as `used_percent: 1`, so any
/// `percent_left >= 99.0` is presented as a full `100`. All other values round to
/// the nearest whole number. Result is clamped to `0..=100`.
fn display_percent_left(percent_left: f64) -> u32 {
    if percent_left >= 99.0 {
        return 100;
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "value is clamped to 0..=100 before the cast"
    )]
    let rounded = percent_left.round().clamp(0.0, 100.0) as u32;
    rounded
}
```

### Step 2: Use the helper in both `write_quota_entry_text` oauth sites

Replace the two rendering blocks so the printed number, the color, and the bar all derive from the
same displayed value. Five-hour:

```rust
if let Some(ref fh) = view.five_hour {
    let shown = display_percent_left(fh.percent_left);
    let ps = percent_style(f64::from(shown));
    writeln!(
        stdout,
        "  Five-hour   {}  {}{}%{} left   resets in {}",
        quota_bar(f64::from(shown), 20, use_color),
        style_open(ps, use_color),
        shown,
        style_close(ps, use_color),
        human_duration_until(fh.reset_at_unix),
    )?;
}
```

Weekly (identical pattern, `wk` / `Weekly`):

```rust
if let Some(ref wk) = view.weekly {
    let shown = display_percent_left(wk.percent_left);
    let ps = percent_style(f64::from(shown));
    writeln!(
        stdout,
        "  Weekly      {}  {}{}%{} left   resets in {}",
        quota_bar(f64::from(shown), 20, use_color),
        style_open(ps, use_color),
        shown,
        style_close(ps, use_color),
        human_duration_until(wk.reset_at_unix),
    )?;
}
```

Note the format string changes from `{:.1}%` to `{}%`, and `shown` (a `u32`) replaces
`fh.percent_left` / `wk.percent_left` as the value argument. Keep the surrounding spacing and
wording byte-for-byte otherwise.

Do NOT touch `write_quota_entry_verbose` or any JSON serialization path.

### Step 3: Unit test the helper

Add a `#[cfg(test)]` test module (or extend an existing one) in `src/ui/mod.rs` asserting the
mapping. Cover the boundary and representative values:

```rust
#[test]
fn display_percent_left_maps_used_percent_to_shown() {
    // percent_left == 100 - used_percent
    assert_eq!(display_percent_left(100.0), 100); // used 0
    assert_eq!(display_percent_left(99.0), 100);  // used 1  -> top bucket
    assert_eq!(display_percent_left(98.0), 98);   // used 2
    assert_eq!(display_percent_left(87.0), 87);   // used 13
    assert_eq!(display_percent_left(65.0), 65);   // used 35
    assert_eq!(display_percent_left(0.0), 0);     // used 100
    // robustness / clamping
    assert_eq!(display_percent_left(105.0), 100);
    assert_eq!(display_percent_left(-3.0), 0);
}
```

If `src/ui/mod.rs` has no existing `mod tests`, add:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    // test above
}
```

### Step 4 (optional but recommended): Integration assertion

In `/workspaces/codex-session/tests/account_quota_cli.rs`, in a test that mounts the wham mock with a
near-full window (e.g. `payload(99.0, 65.0)`), add an assertion that the five-hour line shows the
full value:

```rust
.stdout(predicate::str::contains("100% left"));
```

Keep existing assertions intact; the mock feeds `percent_left` directly via `payload(...)`.

### Final Step: Update plan index

Update the plan's `README.md` (same directory as this round file) to record completion:

1. In the `## Execution Order` table, find the row for round 01.
2. Change `Status` from `todo` to `done`.
3. Change `Completed` from `--` to today's date (`YYYY-MM-DD`).

Because this is the final (and only) round, also:

4. In the README.md header blockquote, change `Status: todo` to `Status: done`.
5. Move the plan directory to done:

```bash
mkdir -p .plan/02-done && mv .plan/01-todo/account-quota-percent-display .plan/02-done/account-quota-percent-display
```

## Acceptance Criteria

- [ ] `display_percent_left` exists in `src/ui/mod.rs`, is pure, and clamps to `0..=100`.
- [ ] A window with `percent_left == 99.0` (i.e. `used_percent: 1`) renders `100% left`.
- [ ] A mid-range window renders the literal whole number (e.g. `percent_left 87.0 -> "87% left"`),
      not shifted up.
- [ ] The printed number, its color (`percent_style`), and the bar (`quota_bar`) all use the same
      displayed value.
- [ ] `write_quota_entry_verbose` and `--format json` output are unchanged (still raw
      `percent_left`).
- [ ] `just test-unit` passes (including the new helper test).
- [ ] `just lint` passes (fmt-check + clippy-strict + print-ownership).
- [ ] Plan `README.md` execution order table shows round 01 as `done` with today's date.
- [ ] Plan `README.md` header status is `done`.
- [ ] Plan directory moved from `.plan/01-todo/account-quota-percent-display` to
      `.plan/02-done/account-quota-percent-display`.

## Next Round

This is the final round.
