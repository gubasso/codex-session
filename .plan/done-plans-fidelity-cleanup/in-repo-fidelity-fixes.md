# Round 01: In-Repo Fidelity Fixes + Divergence Addenda

> Plan: done-plans-fidelity-cleanup | Round: 01 of 02 | Complexity: S
> Generated: 2026-06-02 | Repo: /workspaces/codex-session

## Context

An adversarial review of every completed (`status: done`) plan in `.plan/` (7 plan
areas, 18 rounds) confirmed the substance of each plan is implemented, but found
a handful of genuine gaps and documentation drifts. None are functional
regressions, and several "divergences" are deliberate improvements that must NOT
be reverted. This round closes the real in-repo gaps, reconciles the live CLI
style guide, adds a durable guard for a latent async footgun, and appends a
short dated "Implementation Notes / Divergences" addendum to each affected
done-plan `_README.md` so the as-built truth is recorded without rewriting
history.

The fixes in this round:

1. Clamp `percent_left` when parsing quota windows (defensive; protects the
   soft-knee scoring dominance guarantee).
2. Remove a stale `LRU` reference from a code comment (LRU was removed as a
   resolution source).
3. Add the named `DoctorReport::all_checks()` helper that Plan 04 specified but
   the code inlined as `flat_map` in two places.
4. Add the dedicated 3-account "no A→B→C→A cycling" failover test that the
   Plan 00 README advertised but does not exist.
5. Add a multi-thread guard test for the `runtime::block_on` sync→async bridge
   (which only works under the multi-thread runtime).
6. Reconcile CLI style guide §9b: move `doctor`/`account refresh`/`account add`
   spinners from "Future scope" to "Current scope".
7. Append a divergence addendum to each of the 7 affected done-plan
   `_README.md` files.

## Previous Rounds

This is the first round — no prior rounds.

## Scope of This Round

- **IN scope:** all in-repo code, test, and documentation fixes listed above,
  plus the 7 divergence addenda; running `just lint` and `just test`.
- **OUT of scope:** cross-repo verification (round 02); any regression of
  deliberate improvements (do NOT make `quota::get` async or remove the
  `runtime::block_on` bridge); additive plan-parity cosmetics explicitly ruled
  out — the decorative `── Title ──` doctor group header, the "no resolved
  account to probe" online warn branch, and splitting the three merged doctor
  tests into separately-named tests.

## Current State

### Key Files

- `/workspaces/codex-session/src/services/account/quota.rs` — `parse_window`
  derives `percent_left` with no bounds clamp. Current code:

  ```rust
  let percent_left = window
      .get("percent_left")
      .and_then(Value::as_f64)
      .or_else(|| {
          window
              .get("used_percent")
              .or_else(|| window.get("usedPercent"))
              .and_then(Value::as_f64)
              .map(|used| 100.0 - used)
      })
      .ok_or(QuotaError::ParseMissingWindow(name))?;
  let reset_at_unix = parse_reset_at_unix(window).unwrap_or(0);

  Ok(Window {
      percent_left,
      reset_at_unix,
  })
  ```

- `/workspaces/codex-session/src/ui/mod.rs` — `write_account_list` fallback
  comment near the `current_rendered` check. Current text:

  ```rust
  // If the resolved active account is not one of the listed rows
  // (e.g. a pinned/env/LRU name that is not registered), no row
  // carried the `▸` marker. Surface the selection explicitly so
  // `account list` never silently hides which account is active.
  ```

- `/workspaces/codex-session/src/commands/doctor.rs` — `DoctorReport` has
  `groups: Vec<CheckGroup>` (struct around lines 58–78). `build_report`
  computes summary and next-steps via two inline `flat_map` expressions:

  ```rust
  populate_next_steps(
      groups.iter().flat_map(|group| group.checks.iter()),
      &mut next_steps,
  );

  let summary = summarize(groups.iter().flat_map(|group| group.checks.iter()));
  ```

  `CheckGroup { name: String, checks: Vec<CheckResult> }` is defined just above
  the `DoctorReport` struct.

- `/workspaces/codex-session/src/runtime.rs` — the sync→async bridge. Its doc
  comment already documents the multi-thread requirement; there is no test
  locking the contract:

  ```rust
  pub(crate) fn block_on<F: Future>(fut: F) -> F::Output {
      if let Ok(handle) = tokio::runtime::Handle::try_current() {
          tokio::task::block_in_place(|| handle.block_on(fut))
      } else {
          // ... fallback current-thread runtime ...
      }
  }
  ```

- `/workspaces/codex-session/tests/account_failover_retry.rs` — holds the
  existing 2-account rotation test `auto_retry_rotates_accounts_and_writes_cooldown`
  and the helpers to reuse: `quota_cache(five_hour, weekly)`, `latest_log_file`,
  `support::{TestEnv, fixture_path}`. The test seeds accounts with
  `env.seed_account(name, "{\"token\":\"test\"}\n")`, writes caches with
  `env.write_quota_cache(name, &quota_cache(..))`, runs with the `fake-429.sh`
  fixture via `CODEX_SESSION_CHILD_BIN`, and parses child attempts from stderr
  lines prefixed `marker:codex-session-fake-429 home=`.

- `/workspaces/codex-session/docs/design/cli-style-guide.md` — §9b
  "Spinners & live progress narration". Current scope lists only
  `account health` (non-`--fast`) and `account quota`; "Future scope" still
  lists `doctor`, `account refresh`, and `account add`:

  ```text
  Future scope:

  - `doctor`, as rolling single-line progress.
  - `account refresh` and `account add`, before and after interactive login, never
    during the native login flow.
  ```

  These shipped in Plan 02 round 03 (e.g. `doctor::run` builds a `SpinnerGroup`).

- `/workspaces/codex-session/.plan/*/_README.md` — the 7 done-plan `_README.md`
  files that receive a divergence addendum (see Step 7).

### Existing Patterns

- Quality gates run through `just` recipes (CLAUDE.md): `just lint`,
  `just test-unit`, `just test-integration`, `just test`, `just check`. Never
  raw cargo.
- Unit tests live in `#[cfg(test)] mod tests` blocks within the source file;
  integration tests live under `tests/` and use the shared `support` module.
- CLI output rules are governed by `docs/design/cli-style-guide.md`.
- Markdown obeys MD040 (fenced blocks need a language) and editorconfig
  even-number indentation.

## Implementation Steps

### Step 1: Clamp `percent_left` in quota parsing

In `/workspaces/codex-session/src/services/account/quota.rs`, in `parse_window`,
clamp the derived `percent_left` to `[0.0, 100.0]` before constructing the
`Window`. This guarantees the soft-knee scoring dominance invariant
(`BELOW_KNEE_PENALTY = 1000.0` exceeding the reachable score spread) holds even
for a malformed API value. Change the binding to:

```rust
let percent_left = percent_left.clamp(0.0, 100.0);
```

inserted immediately after the existing `let percent_left = window … ?;` and
before `let reset_at_unix = …`. Add a `#[cfg(test)]` unit test in this file (or
extend an existing test module) asserting that an input `percent_left` of e.g.
`150.0` and `-20.0` both clamp to `100.0` and `0.0` respectively. Verify no
other code path constructs a `Window` with an unclamped `percent_left` (search
for `Window {`); if one exists, clamp there too or route it through
`parse_window`.

### Step 2: Remove the stale `LRU` comment

In `/workspaces/codex-session/src/ui/mod.rs`, update the `write_account_list`
fallback comment so it no longer references `LRU` (removed as a resolution
source in Plan 00). Change `(e.g. a pinned/env/LRU name that is not registered)`
to `(e.g. a pinned or env name that is not registered)`. Search the whole repo
for any other live (non-`.plan/`) `LRU`/`Lru` references in code or docs and
remove/correct them; the historical done-plan files are handled by the
addendum in Step 7, not edited here.

### Step 3: Add `DoctorReport::all_checks()` and route summary/next-steps through it

In `/workspaces/codex-session/src/commands/doctor.rs`, add a method:

```rust
impl DoctorReport {
    /// Iterate every check across all groups, in group order.
    pub(crate) fn all_checks(&self) -> impl Iterator<Item = &CheckResult> {
        self.groups.iter().flat_map(|group| group.checks.iter())
    }
}
```

Then refactor `build_report` so `summary` and `next_steps` are computed via this
method instead of the two inline `flat_map` expressions. The safe reorder:
construct the `DoctorReport` first with `summary: CheckSummary::default()` and
`next_steps: Vec::new()` (bind it `let mut report = DoctorReport { … }`), then:

```rust
populate_next_steps(report.all_checks(), &mut report.next_steps);
report.summary = summarize(report.all_checks());
```

Preserve identical output ordering and values (group order is unchanged). Add a
`#[cfg(test)]` unit test asserting `all_checks()` yields every check across
groups in order. Existing doctor snapshot/JSON tests
(`tests/cmd_doctor.rs`, `tests/cmd_doctor_spinner.rs`) must still pass unchanged.

### Step 4: Add the 3-account no-recycle failover test

In `/workspaces/codex-session/tests/account_failover_retry.rs`, add a new test
that proves auto-failover never re-picks an already-tried account across three
accounts. Mirror `auto_retry_rotates_accounts_and_writes_cooldown`: seed three
accounts (e.g. `alpha`, `beta`, `gamma`), write a distinct quota cache for each,
run `exec` under `fake-429.sh` with `--account auto --max-retries 3` (enough to
attempt all three), then collect the `marker:codex-session-fake-429 home=`
lines. Assert:

- exactly 3 child attempts occurred;
- the 3 attempted `CODEX_HOME` values are all distinct (no account repeated) —
  e.g. collect into a `HashSet` and assert `len() == 3`, and explicitly
  `assert_ne!` each pair;
- the command exits `75` with the auto-exhaustion message and a cooldown file
  for each of the three accounts.

Name it descriptively, e.g. `auto_failover_never_recycles_across_three_accounts`.

### Step 5: Add a multi-thread guard test for the runtime bridge

In `/workspaces/codex-session/src/runtime.rs`, add a `#[cfg(test)] mod tests`
block that locks the bridge's contract under the multi-thread runtime (matching
the production `#[tokio::main]` default). Include:

```rust
#[tokio::test(flavor = "multi_thread")]
async fn block_on_runs_future_from_within_multi_thread_runtime() {
    // Calling the sync bridge from inside a multi-thread runtime must work
    // (nested block_in_place). A regression to a current-thread runtime would
    // panic here, catching the footgun documented on `block_on`.
    let out = tokio::task::spawn_blocking(|| super::block_on(async { 21 * 2 }))
        .await
        .unwrap();
    assert_eq!(out, 42);
}
```

Adjust the exact form to compile cleanly (the goal: exercise the
`Handle::try_current()` branch). Optionally, if the project's tokio version
exposes `Handle::runtime_flavor()`, add a `debug_assert!` in the in-runtime
branch of `block_on` asserting `RuntimeFlavor::MultiThread`; skip the
`debug_assert!` if the API is unavailable rather than pinning a new dependency
version.

### Step 6: Reconcile CLI style guide §9b

In `/workspaces/codex-session/docs/design/cli-style-guide.md` §9b, move `doctor`,
`account refresh`, and `account add` from "Future scope" into "Current scope"
(they shipped in Plan 02 round 03), keeping the qualifiers (doctor as rolling
single-line progress; refresh/add before and after interactive login, never
during native login flow). If "Future scope" becomes empty, remove the heading
or replace it with the genuine remaining future item (a progress-aware tracing
writer, already mentioned later in §9b). Keep the suppression matrix and color
gating text intact.

### Step 7: Append divergence addenda to the 7 affected done-plan `_README.md` files

To each done-plan `_README.md` below, append a clearly-labeled section titled
`## Implementation Notes / Divergences (added 2026-06-02)` recording where the
as-built code intentionally diverged from the plan. Do NOT edit the original
acceptance criteria or round text. Content per file:

- `/workspaces/codex-session/.plan/account-auto-default-selection/_README.md`
  - `run_auto` behaviors are covered by integration tests
    (`tests/account_failover_*.rs`), not unit tests inside `retry.rs`.
  - The usability helper is named `unusable_reason` (not the plan's
    `skip_reason`); same responsibility.
  - The `NoneSelected` message wording differs slightly from the plan; semantics
    are equivalent and `account use` is correctly absent.
  - The dedicated 3-account no-recycle test the README advertised was missing and
    is added by plan `done-plans-fidelity-cleanup` round 01.
  - A stale `LRU` code comment in `src/ui/mod.rs` was corrected in the same fix
    plan.

- `/workspaces/codex-session/.plan/cli-design-system-colorful-output/_README.md`
  - The doctor renderer uses a grouped layout with `✓/⚠/✗` symbols, superseding
    the columnar `STATUS/CHECK/DETAIL` table the plan prose described; the design
    guide SoT was updated to match.
  - Plan prose carries stale line numbers (the codebase grew after writing).
  - Health status tokens render with spaces (`"cache only"`, `"cache missing"`),
    not the underscore forms the spec table showed.
  - §9b "Future scope" for doctor/refresh/add spinners was reconciled to
    "Current scope" by the fix plan.

- `/workspaces/codex-session/.plan/spinner-parallel-async-ux/_README.md`
  - **Architecture divergence (intentional, kept):** `quota::get` stayed
    synchronous; a new `src/runtime.rs` `block_on` sync→async bridge
    (`block_in_place`, multi-thread-only) was introduced, and `pass_through::run`
    is wrapped in `tokio::task::block_in_place` by dispatch. The plan said
    `quota::get` would become `async` and the exec path would call it without
    `.await`; the shipped design keeps the exec hot path sync, which is better.
  - `indicatif` is `0.18` (not `0.17`); `console 0.16` was added for the spinner
    draw target.
  - `token_rotated`/`read_access_token` live in a shared
    `src/services/account/online_probe.rs`, not colocated in `health.rs`.
  - A multi-thread guard test for the `block_on` bridge was added by the fix plan.

- `/workspaces/codex-session/.plan/quota-soft-gate-and-messaging/_README.md`
  - The `account quota` **summary** table does not visually mark below-knee
    accounts as "deprioritized"; only the detailed scoring view surfaces it. No
    blocking/"ineligible" language appears, so the contract holds.
  - `percent_left` was unclamped at parse time; the fix plan clamps it to
    `[0, 100]` to make the scoring dominance guarantee robust to malformed input.
  - The empty-report branch of `no_eligible_detail` is unreachable from the live
    auto path (the user-facing `NoEligible` always carries a populated report).

- `/workspaces/codex-session/.plan/doctor-refactor-completeness-ux/_README.md`
  - `DoctorReport::all_checks()` was specified but not implemented (code inlined
    `flat_map`); the fix plan adds the named helper.
  - `run_online_checks` delegates to `online_probe::probe_and_quota` rather than
    containing a literal `block_on(tokio::join!)` — an improvement (stronger race
    mitigation); the literal AC wording is therefore not met as written.
  - Three R02 tests exist only under merged names
    (`doctor_text_piped_output_is_clean`, `doctor_text_output_snapshot`), not the
    plan's `doctor_piped_output_no_ansi` / `_has_section_headers` /
    `_has_summary_banner`.
  - The decorative `── Title ──` group header and the "no resolved account to
    probe" online warn branch were not implemented; current behavior accepted.

- `/workspaces/codex-session/.plan/configs-rename-split-profiles/_README.md`
  - `FileConfigRecipeConfig.profiles_dir` uses snake_case (Figment consistency),
    not the plan's `#[serde(rename = "profiles-dir")]`.
  - The enum variant is spelled `Unparseable` (vs the plan's `Unparsable`);
    behavior/exit code unchanged.
  - `cache_config_target` returns `cache_dir.join("configs.toml")` — `cache_dir`
    already includes the app segment, so the plan's
    `codex-session/configs.toml` literal was stale.
  - Round 05 is cross-repo (dotfiles); only its on-disk end-state is verifiable
    from this repo. Some `upstream-codex.md` per-section "Last verified" stamps
    were not bumped with the file header.

- `/workspaces/codex-session/.plan/prex-sandbox-fix-tmpdir-migration/_README.md`
  - This plan is ~95% cross-repo. Only round 01's F15 entry in
    `docs/upstream-codex.md` is in-repo and verified; rounds 02–03
    (prex/skills/dctl, conventions docs) live in `~/.claude/skills`,
    `~/.dotfiles`, `~/DocsNNotes` and are not verifiable from this repository.
    Their status is audited by plan `done-plans-fidelity-cleanup` round 02.

### Step 8: Run quality gates

Run `just lint` and `just test`. Fix any failures introduced by the steps above.
Do not use raw cargo.

### Final Step: Update the queue

In this plan's `_QUEUE.yaml`, set the `in-repo-fidelity-fixes` round's `status`
to `done`. (The plan stays `todo` in the top-level `.plan/_QUEUE.yaml` until the
`cross-repo-verification` round also completes.)

## Acceptance Criteria

- [ ] `parse_window` clamps `percent_left` to `[0, 100]`, with a unit test
      covering above-100 and below-0 inputs; no other unclamped `Window`
      construction remains.
- [ ] No live (non-`.plan/`) `LRU`/`Lru` reference remains in code or docs; the
      `write_account_list` comment is corrected.
- [ ] `DoctorReport::all_checks()` exists and both `summarize` and
      `populate_next_steps` route through it; a unit test covers it; existing
      doctor snapshot/JSON tests pass unchanged.
- [ ] A 3-account failover test asserts exactly 3 distinct attempted
      `CODEX_HOME`s (no recycling), exit 75, and a cooldown per account.
- [ ] A `#[tokio::test(flavor = "multi_thread")]` test exercises
      `runtime::block_on` from within a runtime.
- [ ] CLI style guide §9b lists `doctor`/`account refresh`/`account add` under
      current (not future) scope.
- [ ] All 7 listed done-plan `_README.md` files have an
      `## Implementation Notes / Divergences (added 2026-06-02)` section; original
      content is unchanged.
- [ ] `just lint` and `just test` pass.
- [ ] This plan's `_QUEUE.yaml` shows the `in-repo-fidelity-fixes` round as
      `done`.

## Next Round

Round 02 performs a read-only audit of the three external repositories that hold
the cross-repo work (`~/.claude/skills`, `~/.dotfiles`, `~/DocsNNotes`) for
Plan 05 round 05 and Plan 06 rounds 02–03, and records a findings report. It has
no dependency on this round's code changes.
