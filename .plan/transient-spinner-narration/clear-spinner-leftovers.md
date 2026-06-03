# Clear spinner leftovers — wipe ✓/✗ narration before final output

> Plan: transient-spinner-narration | Round: 1 of 1 | Complexity: S | Generated: 2026-06-03 |
> Repo: /workspaces/codex-session

## Context

`codex-session` shows indicatif spinners on stderr while commands do live work (parallel
per-account quota/health fetches, doctor checks, login flows). When a spinner finishes, the
code calls `SpinnerHandle::finish_ok(msg)` / `finish_err(msg)`, which converts the spinner line
into a **persistent** `✓ <msg>` / `✗ <msg>` line. The result for `account quota` with three
accounts:

```text
✓ cwnt
✓ isma
✓ mari                                                                   #1 -946.00 cwnt
  5-hour      ░░░░░░░░░░░░░░░░░░░░  0% left   resets in 1h 22m
  ...
```

The `✓ <account>` lines pollute the report. Spinner narration must be pure wait-time feedback:
transient checkmarks while other lines are still in flight are fine, but once work completes,
all spinner lines must wipe so only the clean report (stdout) remains. The same leftover
pattern exists in `account health`, `doctor`, `account add`, and `account refresh` (where the
stderr `✓ ...` line duplicates the stdout `✓ account added: X` mutation block).

Confirmed decisions: clear immediately when the last task finishes (no artificial delay), and
wipe ✗ error lines too — the report / error renderer owns all durable output (the quota report
renders inline red `Error: ...` entries per failing account; the health report shows
per-account status).

## Previous Rounds

This is the first round — no prior rounds.

## Scope of This Round

IN scope:

- `src/ui/spinner.rs`: un-dead-code `SpinnerGroup::clear()`.
- `src/commands/account/quota.rs`: wipe the spinner group before the report (multi path);
  `finish_and_clear()` in the single-account path (both arms).
- `src/commands/account/health.rs`: wipe the spinner group before the report.
- `src/commands/doctor.rs`: `finish_and_clear()` instead of a persistent finish line; remove
  the now-dead `DoctorFinish` plumbing and its unit tests.
- `src/commands/account/add.rs`, `src/commands/account/refresh.rs`: `finish_and_clear()`
  instead of the final `finish_ok(...)`.
- `docs/design/cli-style-guide.md` §9b: document the transient-only completion-marker rule.

OUT of scope:

- Any change to spinner visibility gating (`should_show_spinner` / `spinner_policy`).
- The `suspend` mechanism, `--quiet`/`--silent` wiring, or a progress-aware tracing writer.
- Any artificial delay/"blink hold" before clearing.
- Changing the in-progress message texts or the report renderers in `src/ui/mod.rs`.

## Current State

### Key Files

- `/workspaces/codex-session/src/ui/spinner.rs` — spinner infrastructure. `SpinnerGroup` wraps
  an `indicatif::MultiProgress` (stderr, indicatif 0.18). Relevant pieces:

  ```rust
  /// Clear all visible spinner lines.
  #[allow(dead_code)]
  pub(crate) fn clear(&self) -> std::io::Result<()> {
      self.multi.clear()
  }
  ```

  `SpinnerHandle` has `finish_ok(&self, msg)` / `finish_err(&self, msg)` (persistent marker
  lines), `finish_and_clear(&self)`, and a `Drop` impl that finish-and-clears unfinished bars.
  Keep `finish_ok`/`finish_err` and their message-helper unit tests — they still render the
  transient in-group markers below.

- `/workspaces/codex-session/src/commands/account/quota.rs` — single-account path:

  ```rust
  match result.await {
      Ok(view) => {
          spinner.finish_ok(target.as_str());
          entries.push(view);
      }
      Err(err) => {
          spinner.finish_err(&format!("{target} — {err}"));
          return Err(err.into());
      }
  }
  ```

  Multi-account path — per-task markers inside `JoinSet::spawn`, then the join loop:

  ```rust
  match &result {
      Ok(_) => spinner.finish_ok(account.as_str()),
      Err(err) => spinner.finish_err(&format!("{account} — {err}")),
  }
  ```

  ```rust
          while let Some(result) = set.join_next().await {
              entries.push(/* ... */);
          }
      }

      entries.sort_by(quota_sort_key);
  ```

  Errors in multi mode do NOT abort: `fetch_view` returns an `AccountQuotaEntryView` with
  `mode: "error"` and the text report renders an inline red `Error: ...` block for it.

- `/workspaces/codex-session/src/commands/account/health.rs` — same shape; per-task markers:

  ```rust
  if view.status == "live" && view.token == "ok" {
      spinner.finish_ok(&view.account);
  } else if view.status == "live" {
      spinner.finish_err(&format!("{} — token {}", view.account, view.token));
  } else {
      spinner.finish_err(&format!("{} — {}", view.account, view.status));
  }
  ```

  followed by `while let Some(result) = set.join_next().await { entries.push(...) }`, then
  `entries.sort_by(...)` and `ctx.ui.write_account_health(...)`.

- `/workspaces/codex-session/src/commands/doctor.rs` — single rolling spinner:

  ```rust
  let spinner = spinners.add("Running checks...");
  let report = build_report(ctx, args, Some(&spinner));
  match doctor_finish(&report.summary) {
      DoctorFinish::Ok(message) => spinner.finish_ok(&message),
      DoctorFinish::Err(message) => spinner.finish_err(&message),
  }
  ctx.ui.write_doctor(&report, fmt)?;
  ```

  Supporting code that becomes dead once the match is removed:

  ```rust
  #[derive(Debug, PartialEq, Eq)]
  enum DoctorFinish {
      Ok(String),
      Err(String),
  }

  fn doctor_finish(summary: &CheckSummary) -> DoctorFinish {
      match (summary.fail, summary.warn) {
          (fail, _) if fail > 0 => DoctorFinish::Err(format!("{fail} checks failed")),
          (_, warn) if warn > 0 => DoctorFinish::Ok(format!("All checks passed ({warn} warnings)")),
          _ => DoctorFinish::Ok("All checks passed".to_owned()),
      }
  }
  ```

  plus three unit tests in `mod tests`: `doctor_finish_reports_failures_as_error`,
  `doctor_finish_reports_warning_count_as_success`, `doctor_finish_reports_clean_success`.
  Note `SpinnerHandle` stays imported — `set_progress`/`build_report` still use it.

- `/workspaces/codex-session/src/commands/account/add.rs`:

  ```rust
  let spinner = spinners.add("Saving account...");
  super::persist_auth_to_seed(&auth_path, &registry, &args.name)?;
  registry.set_current(&args.name)?;
  spinner.finish_ok(&format!("Account \"{}\" added", args.name));
  ```

  Immediately after, `ctx.ui.write_account_mutation("added", ...)` prints the durable stdout
  block (`✓ account added: X` + dim path line). The earlier
  `spinner.finish_and_clear_for_child()` before the interactive login stays as-is.

- `/workspaces/codex-session/src/commands/account/refresh.rs`:

  ```rust
  let spinner = spinners.add("Saving credentials...");
  super::persist_auth_to_seed(&auth_path, &registry, &name)?;
  registry.delete_group_auths(&name)?;
  spinner.finish_ok("Credentials refreshed");
  ```

  Same: `write_account_mutation("refreshed", ...)` follows. The earlier
  `finish_and_clear_for_child()` stays.

- `/workspaces/codex-session/docs/design/cli-style-guide.md` §9b ("Spinners & live progress
  narration") — currently says:

  ```text
  Completion markers render as `✓ <msg>` in `GREEN` for success and `✗ <msg>` in
  `RED` for failure. When color is disabled, use `[ok] <msg>` and `[err] <msg>`
  ASCII fallbacks. Finished spinner lines render as marker plus message only; the
  spinner style must switch to `{msg}` before finishing. Transient spinners may
  finish-and-clear when no residual progress line is useful.
  ```

### Existing Patterns

- Spinners are stderr-only; command results (text tables / JSON) are stdout-only.
- Spinners only render when stderr is a TTY (`should_show_spinner`), so integration tests
  (piped) never see them — `tests/cmd_doctor_spinner.rs` only asserts absence of frames/ANSI
  on pipes and is unaffected.
- Quality gates: `just lint` (fmt-check + clippy-strict + print-ownership), `just test-unit`,
  `just test-integration`. Do not use raw `cargo` for verification.
- The style guide is the source of truth for CLI output behavior; code changes to renderers
  must be mirrored there.

## Implementation Steps

### Step 1: Un-dead-code `SpinnerGroup::clear()` (`src/ui/spinner.rs`)

Remove the `#[allow(dead_code)]` attribute from `SpinnerGroup::clear()` (it becomes used in
Steps 2–3). Leave `SpinnerGroup::suspend` and `SpinnerHandle::clear` (still unused) and all
existing unit tests untouched.

### Step 2: Wipe the quota spinner group before the report (`src/commands/account/quota.rs`)

- Multi-account path: keep the per-task `finish_ok`/`finish_err` calls (transient live
  feedback), and after the `while let Some(result) = set.join_next().await { ... }` loop —
  i.e. once all tasks have finished — wipe the group before `entries.sort_by(quota_sort_key);`:

  ```rust
  let _ = spinners.clear();
  ```

  Place it so it runs for the multi path (inside the `else` block, after the join loop).

- Single-account path: replace both finish calls with `spinner.finish_and_clear();` —

  ```rust
  match result.await {
      Ok(view) => {
          spinner.finish_and_clear();
          entries.push(view);
      }
      Err(err) => {
          spinner.finish_and_clear();
          return Err(err.into());
      }
  }
  ```

  (On error the command aborts and `error.rs::render()` prints the failure; no spinner residue.)

### Step 3: Wipe the health spinner group before the report (`src/commands/account/health.rs`)

Keep the per-task `finish_ok`/`finish_err` calls. After the
`while let Some(result) = set.join_next().await { ... }` loop and before
`entries.sort_by(...)`, add:

```rust
let _ = spinners.clear();
```

### Step 4: Doctor spinner clears instead of leaving a summary line (`src/commands/doctor.rs`)

Replace the finish match in `run()`:

```rust
let report = build_report(ctx, args, Some(&spinner));
spinner.finish_and_clear();
ctx.ui.write_doctor(&report, fmt)?;
```

Then delete the now-dead `DoctorFinish` enum, the `doctor_finish()` function, and the three
unit tests `doctor_finish_reports_failures_as_error`,
`doctor_finish_reports_warning_count_as_success`, `doctor_finish_reports_clean_success`
(clippy-strict fails on dead code otherwise). Keep `set_progress` and the `SpinnerHandle`
import — `build_report` still drives the rolling progress messages.

### Step 5: Mutation commands clear their final spinner (`add.rs`, `refresh.rs`)

- `/workspaces/codex-session/src/commands/account/add.rs`: replace
  `spinner.finish_ok(&format!("Account \"{}\" added", args.name));` with
  `spinner.finish_and_clear();`.
- `/workspaces/codex-session/src/commands/account/refresh.rs`: replace
  `spinner.finish_ok("Credentials refreshed");` with `spinner.finish_and_clear();`.

The stdout `write_account_mutation` block is the durable confirmation in both commands. The
earlier `finish_and_clear_for_child()` calls (before interactive login) stay unchanged.

### Step 6: Update the style guide (`docs/design/cli-style-guide.md` §9b)

Rewrite the completion-marker paragraph (quoted under Current State) to state the new
contract:

- Completion markers `✓ <msg>` (GREEN) / `✗ <msg>` (RED), ASCII `[ok]`/`[err]` when plain, are
  **transient**: they may appear while a spinner group is still live (other lines in flight),
  but the group MUST clear before the command writes its final stdout report or returns an
  error. No spinner line persists after the command's durable output; the report / error
  renderer owns all durable output (including per-account error entries).
- Multi-line groups (`account quota`, `account health`) keep per-line ✓/✗ markers as live
  feedback and wipe the whole group once the last line finishes.
- Single-spinner commands (`doctor`, `account add`, `account refresh`, single-account
  `account quota`) finish-and-clear directly.
- Keep the existing rules about marker styling (`{msg}` style switch before finishing) and the
  60-char finish-message limit — they still govern the transient markers.

### Step 7: Verify

1. `just lint` — catches dead code / fmt drift.
2. `just test-unit` and `just test-integration` — spinner-policy and doctor tests must pass
   (the three deleted `doctor_finish_*` tests are gone).
3. Manual TTY check (spinners only render on a real TTY):
   - `just run -- account quota` → spinners + transient ✓ while fetching, then the block wipes
     and only the clean report remains (no `✓ <account>` lines above it).
   - `just run -- account health` and `just run -- doctor` → same: no leftover marker line.
   - `just run -- account quota 2>/dev/null | cat` → report unchanged on stdout.

### Final Step: Update the queue

Record completion in the queue — status lives in YAML; nothing moves on disk:

1. In this plan's `_QUEUE.yaml`
   (`/workspaces/codex-session/.plan/transient-spinner-narration/_QUEUE.yaml`), set this
   round's (`item: clear-spinner-leftovers`) `status` to `done`.
2. All rounds are now done, so in the top-level `.plan/_QUEUE.yaml` set this plan's
   (`item: transient-spinner-narration`) `status` to `done`. Leave the plan directory in
   place.

## Acceptance Criteria

- [ ] On a TTY, `account quota` (multi-account) shows per-account spinners that turn into
      transient ✓/✗ markers, then the whole group wipes before the report prints — zero
      spinner-residue lines above the report.
- [ ] Same for `account health`; `doctor`, `account add`, `account refresh`, and
      single-account `account quota` leave no persistent spinner line (stdout report /
      mutation block / error rendering is the only durable output).
- [ ] Failed accounts still surface: quota report shows inline `Error: ...` entries; health
      report shows per-account status; single-account failures render through the standard
      error renderer.
- [ ] `DoctorFinish`, `doctor_finish()`, and their three unit tests are removed.
- [ ] `#[allow(dead_code)]` removed from `SpinnerGroup::clear()`.
- [ ] `docs/design/cli-style-guide.md` §9b documents the transient-only completion-marker
      contract.
- [ ] `just lint`, `just test-unit`, `just test-integration` all pass.
- [ ] This plan's `_QUEUE.yaml` shows round `clear-spinner-leftovers` as `done`.
- [ ] The top-level `.plan/_QUEUE.yaml` shows this plan as `done`.

## Next Round

This is the final round.
