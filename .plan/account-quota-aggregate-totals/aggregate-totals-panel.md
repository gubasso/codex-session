# Round 01: Aggregate Totals Panel

> Plan: account-quota-aggregate-totals | Round: 01 of 01 | Complexity: S
> Generated: 2026-05-28 | Repo: /workspaces/codex-session

## Context

`codex-session account quota` renders a 5-hour bar and a weekly bar per
account, with reset countdowns. Operators of multi-account failover pools
need a pool-wide view: "how much capacity is left in total right now?"
Today they must eyeball the per-account list and average mentally.

This round adds a footer **TOTAL** panel that prints once after the
per-account list, when ≥ 2 OAuth accounts contributed. The panel reuses
the existing 20-cell `quota_bar()` and `percent_style()` helpers — no new
chart primitive. It shows mean `percent_left` for each window (5-hour,
weekly), DIM header `TOTAL (avg across N accounts)`, no reset countdown
(omitted on purpose — averaging per-account resets is misleading).

A second change rides along: rename the rendered label `Five-hour` →
`5-hour` everywhere the human label appears. The kebab-case JSON field
`five-hour` is unchanged.

A third change is the JSON shape: today `account quota --format json`
emits a bare `[AccountQuotaEntryView, ...]` array. It becomes
`{ entries: [...], aggregate: {...} | null }`. This is a breaking change,
permitted by the pre-v1.0 policy in `CLAUDE.md`. Single-account
invocations also emit the wrapper (with `aggregate: null`) for shape
uniformity.

## Previous Rounds

This is the first (and only) round — no prior rounds.

## Scope of This Round

**In scope:**

- New view structs: `AccountQuotaAggregateWindowView`,
  `AccountQuotaAggregateView`, `AccountQuotaListView`.
- New pure helper function in `src/services/account/quota.rs` (or near the
  command layer if more natural) that computes the aggregate from a slice
  of `AccountQuotaEntryView`.
- Update `src/commands/account/quota.rs run()` to compute aggregate and
  pass it down.
- Update `src/ui/mod.rs`:
  - Rename text label `Five-hour` → `5-hour` at every render site.
  - Extend `write_account_quota` and `write_account_quota_many` to accept
    and serialize the aggregate.
  - New private `write_quota_aggregate_text` renderer for the TOTAL panel.
- JSON wrapper applied to both single-account and multi-account paths.
- Update `tests/account_quota_cli.rs` with predicate assertions for:
  text TOTAL panel, JSON wrapper shape, single-account aggregate-null,
  all-api-key/error pool aggregate-null, mixed-window-error case, and
  label rename.
- Update existing tests that assert literal `Five-hour` → `5-hour`.
- Update `docs/design/cli-style-guide.md` §14 `account quota` to document
  the TOTAL footer and the label rename.

**Out of scope:**

- Migrating `AccountQuotaWindowView.reset_at_unix` to `Option<u64>` (kept
  unchanged; aggregate uses a separate struct).
- Renaming JSON field `five-hour` (only the human label changes).
- Capacity-out-of-N framing, earliest/latest reset display, opt-in
  `--summary` flag — all rejected during planning.
- Any change to `account quota --account NAME` text rendering (still no
  TOTAL panel for single-account text; only JSON wrapper applies).
- Refactoring `quota_bar()` / `percent_style()` — reuse as-is.

## Current State

### Key Files

- `/workspaces/codex-session/src/commands/account/mod.rs` — view-type
  module shared by all `account *` subcommands.

  Existing types around lines 64–86:

  ```rust
  pub(crate) struct AccountQuotaWindowView {
      pub(crate) percent_left: f64,
      pub(crate) reset_at_unix: u64,
  }

  pub(crate) struct AccountQuotaEntryView {
      pub(crate) account: String,
      pub(crate) active: bool,
      pub(crate) mode: String,            // "oauth" | "api-key" | "error"
      pub(crate) fetched_at_unix: u64,
      pub(crate) ttl_secs: u64,
      pub(crate) error: Option<String>,
      pub(crate) five_hour: Option<AccountQuotaWindowView>,
      pub(crate) weekly: Option<AccountQuotaWindowView>,
      pub(crate) score: Option<f64>,
      pub(crate) rank: Option<usize>,
      pub(crate) status_label: String,
      pub(crate) scoring: Option<AccountScoringView>,
  }
  ```

  Both derive `serde::Serialize` with `#[serde(rename_all = "kebab-case")]`.

- `/workspaces/codex-session/src/commands/account/quota.rs` — command
  entry point. `run()` lives at lines 10–86. It loads accounts, calls
  `fetch_view()` for each, sorts by score, assigns ranks, then dispatches
  to either `write_account_quota` (single account) or
  `write_account_quota_many` (multi).

- `/workspaces/codex-session/src/services/account/quota.rs` — quota
  service. The per-account `Quota` / `Window` types live at lines 21–31
  and back the view at the command layer.

- `/workspaces/codex-session/src/ui/mod.rs` — rendering layer. Key
  functions:

  - `write_account_quota` at lines 395–418 (single-account, text/JSON).
  - `write_account_quota_many` at lines 421–452 (multi-account, text/JSON).
  - `write_quota_entry_text` at lines 820–896 — branches on `view.mode`
    ("oauth" / "api-key" / other). The OAuth branch renders the
    `Five-hour` and `Weekly` lines at lines 836 and 848.
  - `write_quota_entry_verbose` at lines 742–778 — verbose key:value mode.
    Has labels `five_hour_pct` / `weekly_pct` — these are kebab-ish keys,
    not the human "Five-hour" label, so they stay.
  - `write_quota_oauth_header` at lines 780–818 — the `#1 12.50 cwnt
    (active)` line above the bars.
  - `quota_bar(pct, width, use_color)` at lines 723–740 — 20-cell bar.
  - `percent_style(pct)` at lines 713–721 — `BOLD_GREEN` (>50%),
    `BOLD_YELLOW` (>20%), `BOLD_RED` (≤20%).
  - DIM / BOLD / BOLD_CYAN style constants live in the `quota_styles`
    module within this file.

  The current OAuth text render (verbatim from line 836):

  ```rust
  writeln!(
      stdout,
      "  Five-hour   {}  {}{:.1}%{} left   resets in {}",
      quota_bar(fh.percent_left, 20, use_color),
      style_open(ps, use_color),
      fh.percent_left,
      style_close(ps, use_color),
      human_duration_until(fh.reset_at_unix),
  )?;
  ```

- `/workspaces/codex-session/tests/account_quota_cli.rs` — predicate-based
  integration tests for the CLI. No snapshot files in tree for quota
  output. Sibling test files in `tests/account_quota_*.rs` exist for
  other concerns (cache, http, basic, etc.) and may also assert on the
  `Five-hour` literal — search and update.

- `/workspaces/codex-session/docs/design/cli-style-guide.md` §14 — source
  of truth for CLI rendering. Lines 365–394 cover `account quota`. The
  example output at line 371 uses the `Five-hour` literal that needs
  updating.

### Existing Patterns

- All view structs derive `#[derive(Debug, Clone, serde::Serialize)]` with
  `#[serde(rename_all = "kebab-case")]`. New aggregate structs match.
- All text rendering takes `use_color: bool` and uses `style_open` /
  `style_close` helpers around `quota_styles::*` style constants.
- JSON output is emitted via `write_json_line(&mut stdout, view)` (see
  lines 416 and 450). The new wrapper struct serializes through the same
  helper.
- Multi-account text mode prints a per-entry "fetched … ago" DIM footer
  line between entries (lines 439–446). The TOTAL panel comes AFTER all
  per-entry blocks, separated by one blank line.
- Pre-v1.0 breaking-change policy from `CLAUDE.md` "Breaking Changes
  Policy" — no compat shims, just migrate directly.

## Implementation Steps

### Step 1: Add aggregate view structs to `src/commands/account/mod.rs`

After the existing `AccountQuotaEntryView` (around line 86), add:

```rust
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct AccountQuotaAggregateWindowView {
    pub(crate) percent_left: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct AccountQuotaAggregateView {
    pub(crate) accounts_counted: usize,
    pub(crate) five_hour: Option<AccountQuotaAggregateWindowView>,
    pub(crate) weekly: Option<AccountQuotaAggregateWindowView>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct AccountQuotaListView {
    pub(crate) entries: Vec<AccountQuotaEntryView>,
    pub(crate) aggregate: Option<AccountQuotaAggregateView>,
}
```

Notes:

- `accounts_counted` is the count of OAuth accounts that contributed to
  at least one window mean.
- Each window in `AccountQuotaAggregateView` is `Option` because a window
  may have zero contributors (e.g., every account's weekly fetch errored
  but 5-hour succeeded).
- `AccountQuotaListView` is the new top-level JSON payload — replaces the
  bare-array shape.

### Step 2: Add `aggregate_windows` helper

Add a pure function next to the existing per-account view-building code.
Recommended location: a new helper in `src/commands/account/quota.rs`
(close to where `run()` builds the entries), since it operates on the
command-layer view type, not the service-layer `Quota` struct. Keep it
private to the module.

```rust
pub(crate) fn aggregate_windows(
    entries: &[AccountQuotaEntryView],
) -> Option<AccountQuotaAggregateView> {
    let oauth: Vec<&AccountQuotaEntryView> = entries
        .iter()
        .filter(|e| e.mode == "oauth")
        .collect();
    if oauth.len() < 2 {
        return None;
    }

    fn mean<F>(rows: &[&AccountQuotaEntryView], pick: F)
        -> Option<AccountQuotaAggregateWindowView>
    where
        F: Fn(&AccountQuotaEntryView) -> Option<f64>,
    {
        let vals: Vec<f64> = rows.iter().filter_map(|r| pick(r)).collect();
        if vals.is_empty() {
            return None;
        }
        let sum: f64 = vals.iter().sum();
        #[allow(clippy::cast_precision_loss)]
        Some(AccountQuotaAggregateWindowView {
            percent_left: sum / vals.len() as f64,
        })
    }

    let five_hour = mean(&oauth, |e| e.five_hour.as_ref().map(|w| w.percent_left));
    let weekly = mean(&oauth, |e| e.weekly.as_ref().map(|w| w.percent_left));

    if five_hour.is_none() && weekly.is_none() {
        return None;
    }

    Some(AccountQuotaAggregateView {
        accounts_counted: oauth.len(),
        five_hour,
        weekly,
    })
}
```

Add unit tests in the same file (or in a `#[cfg(test)] mod tests` block)
covering: 2+ OAuth accounts mean; 1 OAuth account → None; mixed
api-key/oauth filters correctly; window-mismatch (some entries missing
weekly) still produces a mean from the rest.

### Step 3: Wire aggregate into `run()`

In `src/commands/account/quota.rs run()` (lines 10–86), after the
sort/rank step and before dispatching to UI, compute:

```rust
let aggregate = aggregate_windows(&entries);
```

Update the dispatch:

- Single-account path: pass `aggregate: None` (the single entry is the
  whole pool of the call; no aggregate makes sense).
- Multi-account path: pass `aggregate` through.

The signature shape:

```rust
ui.write_account_quota_many(&entries, aggregate.as_ref(), format, verbose)?;
ui.write_account_quota(&view, format, verbose)?;       // single
```

### Step 4: Rename `Five-hour` → `5-hour` (text label only)

In `src/ui/mod.rs`:

- Line ~836 in `write_quota_entry_text`: change the format-string literal
  `"  Five-hour   {}  …"` to `"  5-hour      {}  …"`. Bump the trailing
  whitespace so the bar column stays aligned with `Weekly` (currently the
  `Weekly` label is `Weekly` with 6 trailing spaces; `5-hour` is one
  character shorter than `Five-hour`, so its padding grows by one space
  to `5-hour`). Verify visually that the bar columns line up.

DO NOT change:

- The `five_hour_pct:` / `weekly_pct:` keys in `write_quota_entry_verbose`
  (those are debug labels, not human chart labels).
- The JSON field name `five-hour` (kebab-case serde rename).
- The struct field name `five_hour`.

Grep the workspace for any other occurrence of the literal string
`Five-hour` and update each (likely candidates: tests that assert on
output text).

### Step 5: Extend `write_account_quota_many` and add aggregate renderer

In `src/ui/mod.rs`:

Update the signature of `write_account_quota_many` (lines 421–452) to
accept the aggregate:

```rust
pub(crate) fn write_account_quota_many(
    &self,
    views: &[crate::commands::account::AccountQuotaEntryView],
    aggregate: Option<&crate::commands::account::AccountQuotaAggregateView>,
    format: crate::cli::OutputFormat,
    verbose: bool,
) -> std::io::Result<()>
```

Text branch — after the existing per-entry loop, if `aggregate.is_some()`
and `!verbose` (the TOTAL panel is for the standard text view; verbose
mode keeps its key:value listing without aggregate), emit a blank line
and call a new helper:

```rust
writeln!(stdout)?;
write_quota_aggregate_text(&mut stdout, agg, c)?;
```

JSON branch — replace `write_json_line(&mut stdout, views)` with:

```rust
let payload = crate::commands::account::AccountQuotaListView {
    entries: views.to_vec(),
    aggregate: aggregate.cloned(),
};
write_json_line(&mut stdout, &payload)
```

Update `write_account_quota` (single-account, lines 395–418) JSON branch
to also emit the wrapper:

```rust
let payload = crate::commands::account::AccountQuotaListView {
    entries: vec![view.clone()],
    aggregate: None,
};
write_json_line(&mut stdout, &payload)
```

Single-account text branch stays unchanged (no TOTAL panel for a single
account).

Add the new private helper:

```rust
fn write_quota_aggregate_text(
    stdout: &mut impl std::io::Write,
    agg: &crate::commands::account::AccountQuotaAggregateView,
    use_color: bool,
) -> std::io::Result<()> {
    writeln!(
        stdout,
        "{}TOTAL (avg across {} accounts){}",
        style_open(quota_styles::DIM, use_color),
        agg.accounts_counted,
        style_close(quota_styles::DIM, use_color),
    )?;
    if let Some(ref fh) = agg.five_hour {
        let ps = percent_style(fh.percent_left);
        writeln!(
            stdout,
            "  5-hour      {}  {}{:.1}%{}",
            quota_bar(fh.percent_left, 20, use_color),
            style_open(ps, use_color),
            fh.percent_left,
            style_close(ps, use_color),
        )?;
    }
    if let Some(ref wk) = agg.weekly {
        let ps = percent_style(wk.percent_left);
        writeln!(
            stdout,
            "  Weekly      {}  {}{:.1}%{}",
            quota_bar(wk.percent_left, 20, use_color),
            style_open(ps, use_color),
            wk.percent_left,
            style_close(ps, use_color),
        )?;
    }
    Ok(())
}
```

Note: no `left`, no `resets in …` on the aggregate row — these are
intentionally omitted (resets are per-account; averaging them is
misleading).

### Step 6: Update callers in `src/commands/account/quota.rs`

`run()` dispatches to `ui.write_account_quota_many(...)`. Update the call
site to pass `aggregate.as_ref()` as the new second argument. Verify the
single-account `ui.write_account_quota(...)` call site compiles (the
single-account signature does NOT take an aggregate parameter, but its
JSON branch now emits the wrapper internally).

### Step 7: Update existing tests

In `tests/account_quota_cli.rs` and any sibling `tests/account_quota_*.rs`
that asserts on text output:

- Grep for the literal `Five-hour` and replace with `5-hour`.
- Grep for any test that does `serde_json::from_str::<Vec<...>>(&out)` on
  `account quota --format json` output — update to parse the new
  `AccountQuotaListView` wrapper shape.

### Step 8: New tests for the TOTAL panel

Add to `tests/account_quota_cli.rs` (predicate-style, matching the file's
existing convention):

1. **Multi-account text** (≥ 2 OAuth accounts): assert output contains
   the literal `TOTAL (avg across`, a subsequent line matching `5-hour`
   followed by a bar, and a `Weekly` line followed by a bar.
2. **Multi-account JSON**: parse output as `serde_json::Value`, assert
   `obj.entries` is an array, `obj.aggregate.accounts-counted` equals the
   expected count, and `obj.aggregate.five-hour.percent-left` is
   approximately the mean of the input `percent_left` values
   (`(a - b).abs() < 0.01`).
3. **Single-account JSON**: `obj.entries` has length 1 and
   `obj.aggregate` is `null`.
4. **Single-account text**: TOTAL panel is absent.
5. **All-api-key pool**: `obj.aggregate` is `null`; text mode shows no
   TOTAL panel.
6. **Mixed window failure**: 3 OAuth accounts where account A's `weekly`
   is `None` (e.g., partial fetch failure); assert
   `obj.aggregate.five-hour.percent-left` averages all 3 and
   `obj.aggregate.weekly.percent-left` averages 2.

Use the existing test harness fixtures in
`/workspaces/codex-session/tests/support/` if quota fixtures live there;
otherwise follow the pattern in adjacent `tests/account_quota_*.rs` files.

### Step 9: Update `docs/design/cli-style-guide.md` §14

`docs/design/cli-style-guide.md` is the source of truth per `CLAUDE.md`.
Edit §14 `account quota` (lines ~365–394):

- Replace the `Five-hour` literal in the OAuth example (line 371) with
  `5-hour` (note the trailing space adjustment to keep bar
  alignment).
- After the existing OAuth/api-key/error examples, add a "Pool totals"
  subsection showing the footer-summary layout with TOTAL panel:

  ```text
    #1 12.50 cwnt (active)
    5-hour      ███████████████░░░░░  75.0% left   resets in 1h 23m
    Weekly      ██████████░░░░░░░░░░  50.0% left   resets in 2d 4h
    2m ago, live

    #2 11.20 alice
    5-hour      ████████████████░░░░  80.0% left   resets in 4h 12m
    Weekly      ██████████░░░░░░░░░░  50.0% left   resets in 2d 4h
    2m ago, live

  TOTAL (avg across 2 accounts)
    5-hour      ███████████████░░░░░  77.5%
    Weekly      ██████████░░░░░░░░░░  50.0%
  ```

- Add a sentence noting: the TOTAL panel renders only when ≥ 2 OAuth
  accounts contributed; reset countdowns are omitted on aggregate rows by
  design (resets are inherently per-account); the panel header is `DIM`
  and the bars use the same `percent_style` thresholds as per-account
  bars.

### Step 10: Verification

Run the project quality gates per `CLAUDE.md`:

```bash
just test-unit
just test-integration
just lint
```

Then visually confirm by running the binary against a real (or fixture)
pool:

```bash
just run -- account quota
just run -- account quota --format json | jq '.aggregate'
just run -- account quota --account NAME             # aggregate: null
just run -- account quota --format json | jq '.entries[0]'
```

Confirm the bar column alignment between per-account rows and the TOTAL
row (the leading two-space indent and `5-hour` vs `Weekly` padding
should produce flush bar starts).

### Final Step: Update plan index

Update the plan's `README.md` (in the same directory as this round file)
to record completion:

1. In the `## Execution Order` table, find the row for round 01.
2. Change `Status` from `todo` to `done`.
3. Change `Completed` from `--` to today's date (`YYYY-MM-DD`).

Because this is the final round, also:

1. In the README.md header blockquote, change `Status: todo` to
   `Status: done`.
2. Move the plan directory to done:

```bash
mkdir -p .plan/02-done && mv .plan/01-todo/account-quota-aggregate-totals .plan/02-done/account-quota-aggregate-totals
```

## Acceptance Criteria

- [ ] `account quota` text output (multi-account, ≥ 2 OAuth accounts)
      renders a `TOTAL (avg across N accounts)` panel after the
      per-account list, separated by a blank line, with `5-hour` and
      `Weekly` bar rows showing only the percentage on the right (no
      `left`, no reset countdown).
- [ ] `account quota` text output uses the label `5-hour` (not
      `Five-hour`) in every per-account and aggregate row.
- [ ] `account quota --format json` emits an object `{ entries: [...],
      aggregate: {...} | null }`. `aggregate` is `null` when fewer than 2
      OAuth accounts contributed; otherwise contains `accounts-counted`
      and per-window means.
- [ ] Single-account invocations (`account quota --account NAME`) emit
      the same `{ entries, aggregate }` wrapper in JSON with
      `aggregate: null`; text mode shows no TOTAL panel.
- [ ] All-api-key / all-error pool: `aggregate` is `null` in JSON, no
      TOTAL panel in text.
- [ ] Mixed window-failure case: per-window means use independent
      contributor counts (an account missing one window still counts
      toward the other).
- [ ] `docs/design/cli-style-guide.md` §14 reflects the new TOTAL footer
      example and the `5-hour` label.
- [ ] `just test-unit`, `just test-integration`, and `just lint` all pass.
- [ ] Plan `README.md` execution order table shows round 01 as `done`
      with today's date.
- [ ] Plan `README.md` header status is `done`.
- [ ] Plan directory moved from `.plan/01-todo/account-quota-aggregate-totals`
      to `.plan/02-done/account-quota-aggregate-totals`.

## Next Round

This is the final round.
