# Round 03 — Doctor, Refresh, Add Spinners + Refresh-Race Fix + Robust Test Suite

> Round 3 of 3 | Topic: extended spinner coverage + the quota/probe refresh-race correctness fix +
> a guide-aligned test suite + plan completion | Status: todo

## Context

codex-session is a Rust CLI wrapper that manages multi-account credential pooling for the `codex`
binary. After Rounds 01 and 02, the codebase uses tokio async **on the network surface only**
(`account health`/`account quota` + quota service), has a reusable spinner module
(`src/ui/spinner.rs`) implementing the §9b style-guide spec, and runs health/quota in parallel with
multi-spinner feedback. This final round:

1. Fixes the remaining **single-use refresh-token race** between the quota fetch and the heartbeat
   probe (the "inverse race" carried over from Round 02 review — see Step 0). This is a correctness
   item and is done **first**.
2. Extends spinner coverage to `doctor`, `account refresh`, and `account add`, and handles signal /
   error-path cleanup.
3. Replaces the draft's thin test sketches with a **robust, guide-aligned test suite** built on the
   project test pyramid (unit → integration → snapshot), per the CLI-design testing canon
   `~/DocsNNotes/tech/programming/cli-design/08-testing-and-quality/` (`README.md`,
   `testing-strategy.md`, `testing-tools.md`) and the Rust implementation
   `~/DocsNNotes/tech/languages/rust/cli-spec/06-testing-and-quality/testing.md`.
4. Completes the plan and moves it to `.plan/02-done/`.

> **Plan-review notes carried into this round.**
>
> - Per the BLOCKER 1 decision, `doctor`, `account refresh`, `account add`, and `gate::run_login`
>   are **synchronous** and stay that way. Spinners work fine in synchronous functions (indicatif's
>   steady-tick runs on its own thread) — none of the spinner steps require making these handlers
>   async. The draft's `async fn run(...)` signatures for doctor/refresh/add are **not** correct;
>   keep them sync.
> - Round 02 shipped the **quota-wins** half of the refresh-race fix (`build_entry` re-probes
>   against quota's freshly-rotated auth file when `quota::refresh` performs a 401 rotation —
>   `health.rs`). The **probe-wins** half (the inverse race) is still open and is the subject of
>   Step 0.

## Current State

After Rounds 01 and 02:

- Tokio runtime active; only the network surface (health/quota + quota service) is async (doctor,
  refresh, add, pass_through, run_login all remain synchronous — BLOCKER 1 scope).
- `src/ui/spinner.rs` provides `SpinnerGroup` and `SpinnerHandle` implementing the §9b style-guide
  spec, with TTY/format-aware visibility, `should_show_spinner`, `suspend`, and a `Drop` that
  finish-and-clears unfinished bars.
- `account health` runs accounts in parallel (`JoinSet`), each account runs quota+probe concurrently
  (`tokio::join!`), and **re-probes against quota's rotated auth file when quota refreshes**
  (Round 02 Option-1 fix; `health.rs` `build_entry`, helpers `fetch_probe_with_auth` /
  `read_access_token`; `gate::probe_token_with_auth`; `quota::resolve_auth_path` is `pub(crate)`).
- `account quota` runs accounts in parallel with spinners.
- Spinners suppressed in JSON mode and non-TTY contexts.

### The probe / quota refresh boundary (relevant to Step 0)

- `gate::probe_token` reads the **seed** (`registry.group_auth_seed_path(account)`), copies it into
  an isolated `CODEX_HOME` tempdir, and runs `codex exec` there. If the copied access token is
  expired, **the child codex refreshes server-side**, consuming the seed's single-use refresh token —
  but the child writes the rotated token only into the throwaway tempdir, never back to the real
  seed (`src/services/account/gate.rs` `heartbeat_probe`).
- `quota::refresh` → on `401` → `token_refresh::refresh_token(resolve_auth_path(...))` rotates the
  token of whichever auth file it resolves (current_group → newest_group → seed) and atomically
  writes it back (`src/services/account/quota.rs`, `src/services/account/token_refresh.rs`).
- `OpenAI` uses **single-use** refresh tokens (RFC 6749 rotation): the old refresh token is
  permanently invalidated on use (`token_refresh.rs:3-5`).

### doctor command (local checks, no progress indication)

File: `/workspaces/codex-session/src/commands/doctor.rs` — sync `run(ctx, args)`; resolves format
via `ctx.global.format.unwrap_or_default()` (`doctor.rs:83-88`), not an `args.format` field. Builds
a `DoctorReport { checks, summary, next_steps, .. }` of sequential local checks.

### account refresh / add (interactive login flow)

Files: `src/commands/account/refresh.rs`, `src/commands/account/add.rs`. Both call
`gate::run_login()` (sync), which spawns an interactive `codex login`. The user interacts with the
child directly (browser OAuth). After login, the command copies the new auth.json into the account
directory.

### Existing test assets to reuse (do not reinvent)

- `tests/support/mod.rs`: `TestEnv::new()`/`new_empty()`, `cmd()` (hermetic `assert_cmd::Command`
  with `env_clear` + curated env), `std_cmd()`, `seed_account(name, auth_body)`,
  `with_stub_child(cmd, fixture)`, hoisted `pub const TEST_AUTH`.
- `tests/support/quota.rs` (Round 02): `wham_url`, `oauth_auth`, `payload`, `add_oauth_account`,
  delayed/recording responder helper.
- `tests/account_quota_token_refresh.rs`: **the canonical pattern for refresh-race tests** — fakes
  the OAuth token endpoint with `wiremock` via `CODEX_SESSION_TOKEN_ENDPOINT`, counts refresh
  requests, and asserts the **seed has the rotated refresh_token** after the run. Step 0 and Step 9
  reuse this pattern verbatim.
- Fixtures: `fake-401-then-ok.sh`, `fake-401.sh`, `fake-429.sh`, `fake-codex-login.sh`,
  `fake-codex-resume.sh`, `sleep.sh`. `libc = "0.2"` is a production dep (usable from tests).
- `tests/snapshots/` + `insta` (`json` feature) for snapshot assertions.

## Previous Rounds

**Round 01** migrated from blocking to async (network surface only). **Round 02** created
`src/ui/spinner.rs`, parallelized health/quota with multi-spinner, and fixed the quota-wins half of
the refresh race (re-probe on quota rotation).

## Scope of This Round

### In scope

1. **Step 0 (correctness, first): eliminate the quota/probe single-use-refresh-token race** (the
   inverse / probe-wins case) so a health check can never orphan a refreshed token or report a false
   `token=invalid`.
2. Doctor command step-by-step spinner progress.
3. Account refresh spinner during the non-interactive phases of login.
4. Account add spinner during the non-interactive phases of login.
5. Signal handling integration (clean spinner teardown on CTRL-C).
6. Error-path spinner cleanup (spinners never linger on errors).
7. A **robust, guide-aligned test suite**: colocated unit tests for the new pure logic, one
   integration file per new subcommand surface, insta snapshots for doctor output, and direct
   regression tests for both refresh-race directions.
8. Final plan completion (robust README update + move to `.plan/02-done/`).

### Out of scope

- Changing doctor check logic or adding new checks.
- Changing the interactive login flow logic.
- Design-system styling (separate plan).
- Re-architecting auth-file authority beyond what Step 0 needs (the seed-vs-group authority question
  is raised for plan-review in Step 0, but a full redesign is deferred).
- Adding new third-party test tooling that requires dependency changes (`proptest`, `cargo-mutants`)
  — recommended as follow-ups, not gated here (see Step 6).

---

## Step 0: Eliminate the quota/probe single-use-refresh-token race (correctness — do first)

Files: `src/commands/account/health.rs`, `src/services/account/gate.rs` (and possibly
`src/services/account/token_refresh.rs` / `quota.rs` for a shared helper).

### The defect (inverse race)

In `build_entry`, `tokio::join!(fetch_quota, fetch_probe)` runs the quota fetch and the heartbeat
probe concurrently. When the account's access token is **expired**, both paths can attempt a refresh
against the same single-use refresh token:

- **Quota wins** → quota rotates the auth file; the concurrent probe (reading the stale token) gets
  a 401 and reports a false `token=invalid`. **Already fixed in Round 02** (re-probe on rotation).
- **Probe wins** (this step) → the probe's isolated child refreshes server-side first, invalidating
  the refresh token, then quota's `token_refresh` fails on the now-dead token (`quota` reports
  `fetch failed`). The probe child's rotated token is written only into the throwaway tempdir and is
  **never persisted to the real seed** → the seed's refresh token is now permanently dead. Every
  subsequent quota fetch and probe for that account fails until the user manually re-logs in.

The probe-wins case is the more serious one: it is not a transient false negative but **persistent
account breakage** caused by an orphaned single-use token.

### Root cause

Two independent refreshers (`token_refresh::refresh_token` and the probe's internal codex child) can
act on the same single-use token concurrently. No re-probe-after-the-fact can recover a token that a
_child process_ already consumed and discarded.

### Recommended fix — single-flight up-front refresh (root-cause)

Refresh the token **once, before** the concurrent quota∥probe join, so neither consumer ever
triggers a second refresh:

1. In `build_entry` (non-fast only), read the token expiry of the active auth file (reuse the
   existing `read_token_state` / `token_expiry_from_auth`).
2. If the token is **expired or near-expiry**, perform exactly one refresh up-front via the existing
   `token_refresh::refresh_token` (wrapped in `spawn_blocking`, as quota already does), persisting
   the rotated token. This consumes the single-use token exactly once and writes it back atomically.
3. Then run `tokio::join!(fetch_quota, fetch_probe)`. Both now read a fresh, valid token, so neither
   refreshes — the race window is closed structurally, not patched after the fact.
4. The Round 02 re-probe-on-rotation logic becomes defense-in-depth (it should now rarely fire).
   Keep it; it is cheap and harmless.

> **Open plan-review question (must be resolved before implementing).** The probe reads the **seed**
> while quota's `resolve_auth_path` prefers a current/newest **group** auth file. For the up-front
> refresh to make _both_ consumers see the fresh token, confirm which file(s) each reads in the
> `account health` execution context and refresh the authoritative one (most likely the seed; verify
> whether any group auth.json exists during health). If seed and group can diverge, the up-front
> refresh must update the seed (probe's source) **and** the file quota resolves, or the round must
> first establish the seed as the single source of truth. Resolve this against
> [docs/upstream-codex.md](/workspaces/codex-session/docs/upstream-codex.md) and the registry code;
> do not guess.

### Secondary fix — make the probe non-destructive (defense-in-depth)

Even with the single-flight refresh, harden the probe so an unexpected child refresh can never
orphan a token: after the child exits in `heartbeat_probe`, compare `<tmp>/auth.json` to the bytes
copied in; if the child rotated it, **persist the rotated bytes back to `auth_source`** via the
secure-permission writer (`crate::services::auth` / `adapters::fs::atomic_write`). This recovers any
token the probe consumed. Gate it so it only writes on an actual change and only when the new bytes
are valid (non-empty `tokens.access_token`).

> Pick the primary (single-flight) as the contract; add the secondary only if plan-review confirms a
> child can still refresh after the up-front refresh (e.g. a token that expires between refresh and
> probe). Document whichever is chosen so the tests in Step 9 assert the right behavior.

### Acceptance for Step 0

- With an expired token and a faked token endpoint, an `account health` run triggers **exactly one**
  refresh request (single-flight), and the **seed retains a valid (rotated) refresh token**
  afterwards — never a dead one.
- A subsequent quota/probe for that account succeeds (no persistent breakage).
- No false `token=invalid` in either race direction.

---

## Step 1: Add spinner to doctor command

File: `/workspaces/codex-session/src/commands/doctor.rs`

Doctor stays **sync** (Round 01 decision); the spinner runs on indicatif's own tick thread. Resolve
the format the way doctor already does — `ctx.global.format.unwrap_or_default()` (`doctor.rs:88`),
**not** an `args.format` field.

```rust
pub(crate) fn run(ctx: &AppContext, args: DoctorArgs) -> Result<u8, AppError> {
    let fmt = ctx.global.format.unwrap_or_default();
    let spinners = SpinnerGroup::new(spinner::should_show_spinner(/* see Step 7 refactor */));
    let spinner = spinners.add("Running doctor checks…");

    spinner.set_message("Checking codex binary…");
    checks.push(check_codex_binary(ctx));
    spinner.set_message("Checking config recipe…");
    checks.push(check_config_recipe(ctx));
    // … one set_message per check …

    let summary = compute_summary(&checks);
    match (summary.fail, summary.warn) {
        (f, _) if f > 0 => spinner.finish_err(&format!("{f} checks failed")),
        (_, w) if w > 0 => spinner.finish_ok(&format!("All checks passed ({w} warnings)")),
        _ => spinner.finish_ok("All checks passed"),
    }

    ctx.ui.write_doctor(&report, fmt)?;
    // …
}
```

A single rolling-message spinner (doctor checks are sequential + local), not multi-spinner.

## Step 2: Add spinner to account refresh command

File: `/workspaces/codex-session/src/commands/account/refresh.rs`

Refresh stays **sync**; `run_login` stays **sync** (no `.await`). The spinner shows **before** the
interactive login (setup) and **after** it (saving credentials), never **during** it.

```rust
pub(crate) fn run(ctx: &AppContext, args: &RefreshArgs) -> Result<(), AppError> {
    let spinners = SpinnerGroup::new(spinner::should_show_spinner(/* Step 7 */));
    let spinner = spinners.add(&format!("Preparing login for \"{account}\"…"));
    // … setup (resolve paths, validate state) …
    spinner.finish_and_clear();                 // CRITICAL: clear before the interactive child

    let exit_code = gate::run_login(ctx, &opts)?;

    if exit_code == 0 {
        let spinner = spinners.add("Saving credentials…");
        // … copy auth.json, verify …
        spinner.finish_ok("Credentials refreshed");
    }
    Ok(())
}
```

The spinner **must** be cleared before spawning the interactive child, or escape codes corrupt the
child's terminal.

## Step 3: Add spinner to account add command

File: `/workspaces/codex-session/src/commands/account/add.rs` — same pattern as Step 2 (sync;
`Setting up account "name"…` → clear → `run_login` → `Saving account…` → `✓ Account "name" added`).

## Step 4: Ensure spinner cleanup on error paths

`SpinnerHandle::Drop` already finish-and-clears unfinished bars (Round 02). Audit the new doctor /
refresh / add handlers to confirm every early return (`?`) or panic unwinds through a `Drop` that
restores the terminal. Add a colocated unit test for the `Drop` behavior (Step 7).

## Step 5: Signal handling for spinner cleanup

> **MINOR 10 — concrete expectation.** The existing signal machinery
> (`src/adapters/spawner.rs` `install_signal_forwarding` + `SignalGuard`) **forwards**
> SIGINT/SIGTERM/SIGHUP to the spawned `codex` child during pass_through exec. The spinner commands
> install no forwarder and have no child during the spinner phase. The design:
>
> 1. Spinner commands rely on indicatif's `Drop` (Step 4): on SIGINT the default handler terminates
>    the process; indicatif restores the terminal as the `ProgressBar`/`MultiProgress` drops during
>    unwind.
> 2. The pass_through exec path keeps its existing forwarding **untouched** — Round 03 does **not**
>    modify `spawner.rs` or `retry.rs`.
> 3. `account refresh` / `account add` clear the spinner before the interactive child (Steps 2-3),
>    so the child owns the screen during login.

Verify empirically (Step 8, signal test + a manual TTY check). Only if residue is observed, add a
global `cleanup_all_spinners()` to `spinner.rs`; otherwise omit it (don't add dead code).

---

## Step 6: Test discipline & tooling (read before writing any test)

Every test added in Steps 7-9 follows the CLI-design testing canon
`~/DocsNNotes/tech/programming/cli-design/08-testing-and-quality/` (`README.md`,
`testing-strategy.md`, `testing-tools.md`) and the Rust spec
`~/DocsNNotes/tech/languages/rust/cli-spec/06-testing-and-quality/testing.md`:

- **Pyramid:** colocated **unit** tests for pure logic (widest base, Step 7) → **one integration
  file per subcommand** against the real binary (Step 8) → **snapshot** assertions on structured
  output (insta, inside integration). No new E2E tier; the CTRL-C test stays integration-tier and
  bounded.
- **FIRST + DAMP + AAA.** Fast, Independent, Repeatable, Self-validating, Timely. Scenarios are
  Descriptive And Meaningful (read top-to-bottom); DRY only the _mechanics_ (`TestEnv`, fixture
  loaders). Name tests after behavior, not implementation.
- **Isolation is non-negotiable.** Use `TestEnv` (`env_clear` + curated env + tempdir). No real
  network — fake at the boundary with `wiremock` (HTTP) and fixture stub scripts (child codex). No
  real clock — no `sleep` except the one unavoidable, bounded signal test (Step 8), justified
  inline. No shared state, no `env::set_var`, no `chdir`.
- **Fakes over mocks** (Meszaros): `wiremock` is a recording HTTP fake; the codex stub scripts are
  fakes of the child boundary. Assert on **our** observable behavior (stdout/stderr/exit code,
  persisted files, request counts) — not on a library's call shape.
- **Don't test third-party libraries.** Never assert on indicatif/wiremock internals. Apply the
  **import-removal heuristic**: if deleting the fake's setup leaves the test still passing, it tests
  the mock, not our code — rewrite or delete it.
- **Argv-contract where the argv is the contract.** For refresh/add, the fact that the spinner is
  cleared _before_ the child spawns is part of the contract; assert it (no ANSI bleed into the
  child's stream).
- **Snapshots reviewed as code.** Use `insta::assert_json_snapshot!` (json feature is already on)
  for doctor's structured output; never auto-accept in CI.
- **Optional quality follow-ups (note, do not gate):**
  - _Property tests_ (`proptest`, dev-dep change) for invariants: `read_access_token` round-trip
    (`tokens.access_token` extracted iff present and non-empty), `token_rotated(pre, post)` (Step 7),
    and finish-message truncation idempotence. Adopt in a follow-up if the dep is approved.
  - _Mutation testing_ (`cargo-mutants`, nightly/manual) on the Step 0 refresh-coordinator and
    `read_access_token` — AI-written suites trend high-coverage / low-mutation; aim ≥ 60% killed on
    these critical modules. Run manually this round; wire into CI nightly separately.

## Step 7: Unit tests + small testability refactors (pyramid base)

The current code mixes pure policy with environment probes, which the guide says to avoid ("test
behavior, not env"). Extract two pure helpers so the base of the pyramid is unit-testable, then test
them colocated in `#[cfg(test)] mod tests`:

1. **Spinner visibility policy.** Split `should_show_spinner` into a pure
   `fn spinner_policy(format, quiet, silent, suppress, stderr_is_tty, mirror_off) -> bool` plus a
   thin wrapper that supplies `stderr_is_tty = std::io::stderr().is_terminal()` and the mirror state.
   Unit-test `spinner_policy` with **one case per suppression axis** (json hides, non-tty hides,
   quiet hides, silent hides, `--fast`/`suppress` hides, mirror-on hides, all-clear shows). File:
   colocated in `src/ui/spinner.rs`.
2. **Token-rotation detection.** Extract `fn token_rotated(pre: Option<&str>, post: Option<&str>) ->
   bool` (true iff `post.is_some() && post != pre`) from `build_entry`'s inline comparison and
   unit-test it (none→some, some→some-changed, some→same, some→none, none→none). File: colocated in
   `src/commands/account/health.rs`.
3. **`read_access_token`** (Round 02 helper): unit-test Some on valid OAuth json; None on missing
   key, empty string, malformed json, and api-key-mode (no `access_token`). Colocated in `health.rs`.
4. **Doctor summary → finish-marker mapping.** If Step 1 introduces a `fn doctor_finish(summary) ->
   FinishKind` pure helper, unit-test the three branches (fail>0 → err; warn>0 → ok+warnings; clean
   → ok). Otherwise assert the mapping via the integration snapshot in Step 8.
5. **Spinner finish-marker rendering** (`✓ msg` / `[ok] msg`; `✗` / `[err]`; ≤ 60-char truncation):
   colocated in `src/ui/spinner.rs`, one test per color/no-color branch and one for truncation.
6. **`SpinnerHandle::Drop`**: a unit test that drops an unfinished handle on a hidden group and
   asserts no panic / clean finish (behavior, not indicatif internals).

These are `kind(lib) + kind(bin)` tests — they run in the **pre-commit** tier (`just test-unit`).

## Step 8: Integration tests — one file per subcommand surface (+ snapshots)

Real-binary tests via `TestEnv`; `predicates::str::contains` for stdout; reserve exact-equality for
tiny stable strings. Each test gets its own tempdir.

1. **`tests/cmd_doctor_spinner.rs`** (new):
   - `doctor_text_piped_output_is_clean`: `doctor --format text` → stdout contains a check-status
     token (`OK`/`WARN`/`FAIL`) and **no** spinner glyphs (`⠋`/`⠙`) or ANSI (`\x1b`) on stdout.
   - `doctor_json_output_is_valid_json`: `doctor --format json` → stdout parses as
     `serde_json::Value`; stderr has no ANSI.
   - `doctor_text_output_snapshot`: seed a fixed, hermetic state and `insta::assert_snapshot!` the
     stdout (catches accidental report-shape drift, including the spinner's finish line not leaking
     into stdout). Review the snapshot as code.
2. **`tests/account_refresh_spinner.rs`** (new): drive a successful login with a child stub
   (`with_stub_child` + `fake-codex-login.sh` — **confirm it covers a success path before adding a
   new fixture**; only add `fake-codex-login-success.sh` if none does). Use the hoisted `TEST_AUTH`.
   - `refresh_piped_mode_has_no_spinner_artifacts`: piped stderr has no `\x1b` (spinner suppressed,
     and the pre-login clear prevented bleed into the child's stream).
   - `refresh_success_persists_credentials`: assert the account's auth.json is updated (behavior),
     not the spinner.
3. **`tests/account_add_spinner.rs`** (new): mirror refresh — `add_piped_mode_has_no_spinner_artifacts`
   and `add_success_creates_account` (the account dir + auth.json exist afterwards).
4. **Error-path cleanup** — extend `tests/account_health_parallel.rs`:
   `health_with_failing_account_cleans_up_spinner` (one account missing `access_token`): JSON output
   has both accounts (one error status), stderr has no `\x1b`.
5. **CTRL-C** — extend `tests/account_health_parallel.rs`:
   `health_killed_during_fetch_exits_cleanly`. Use `std_cmd()` (not `cmd_raw` — it doesn't exist),
   **non-fast** (so the process is actually blocked on a 30s wiremock delay), `libc::kill(pid,
   SIGINT)` after a bounded 1s, then `child.wait()`; assert **non-success / didn't hang** — **do not
   assert exit code 130** (the spinner commands use the default SIGINT disposition; see Step 5).
   This single bounded sleep is the one justified exception to "no sleep in tests" — annotate it.

## Step 9: Refresh-race correctness tests (lock down Step 0, both directions)

File: `tests/account_health_refresh_race.rs` (new). Reuse the **token-endpoint fake pattern** from
`tests/account_quota_token_refresh.rs` (wiremock at `CODEX_SESSION_TOKEN_ENDPOINT`, plus
`CODEX_SESSION_WHAM_USAGE_URL` for quota) and assert via **request counts and persisted seed state**
(jitter-resistant, like Round 02's arrival-window approach), never tight wall-clock.

1. `expired_token_refreshes_exactly_once` (single-flight, Step 0 primary): seed an account with an
   **expired** access token + valid refresh token; fake the token endpoint to return one rotation
   and **count requests**. Run non-fast `account health`. Assert the token endpoint received
   **exactly 1** refresh request — proving quota and the probe did not both refresh.
2. `expired_token_persists_rotation_to_seed` (no orphaning, inverse-race regression): after the same
   run, read the **real seed** auth.json and assert its `tokens.refresh_token` is the **rotated**
   value, not the original dead one ("seed should have the rotated refresh_token" — exact assertion
   shape as `account_quota_token_refresh.rs:145`). This fails on today's code (token orphaned) and
   passes after Step 0.
3. `expired_token_then_quota_succeeds` (no persistent breakage): after the refresh, a follow-up
   `account quota --format json` for the same account succeeds (the seed's token is alive).
4. `live_token_does_not_refresh` (no regression / no needless rotation): seed a **valid** token; run
   non-fast `account health`; assert the token endpoint received **zero** refresh requests and the
   seed is byte-identical.

> These four tests assert **our** contract (request counts, persisted seed bytes, exit success) —
> apply the import-removal heuristic: deleting the wiremock fakes would make them fail to reach the
> code under test, confirming they cover the real boundary, not a mock.

## Step 10: Review spinner messages for UX quality

Messages must match the §9b style-guide spec (Round 02 Step 0 — the canonical source); this table is
its concrete instantiation. Verify present-participle in-progress / past-tense done, quoted account
names, consistent `✓`/`✗` (color) vs `[ok]`/`[err]` (no-color), ≤ 60 chars.

| Command           | Phase             | Spinner Message                                           |
| ----------------- | ----------------- | --------------------------------------------------------- |
| `account health`  | Per-account check | `Checking account "name"…`                                |
| `account health`  | Account done      | `✓ name` / `✗ name — status`                              |
| `account quota`   | Per-account fetch | `Fetching quota for "name"…`                              |
| `account quota`   | Account done      | `✓ name` / `✗ name — error`                               |
| `doctor`          | Running checks    | `Running checks…` → `Checking codex binary…` → …          |
| `doctor`          | Done              | `✓ All checks passed` / `✗ N checks failed`               |
| `account refresh` | Before / after    | `Preparing login for "name"…` → `✓ Credentials refreshed` |
| `account add`     | Before / after    | `Setting up account "name"…` → `✓ Account "name" added`   |

## Step 11: Run the full gate

```bash
just test-unit          # pyramid base (pre-commit tier)
just test-integration   # per-subcommand (pre-push tier)
just test               # all
just lint
just check              # authoritative final gate
```

All green, no new warnings, no global `#[allow]` papering over lints. If the Step 0 fix or refactors
add/extract a module, confirm `cargo nextest` picks up the new unit tests (`kind(lib)`/`kind(bin)`).

## Step 12: Update the plan index

File: `/workspaces/codex-session/.plan/01-todo/02-spinner-parallel-async-ux/README.md`

Set the Round 03 row's `Status` `todo → done` and fill `Completed` with today's date. Also flip the
plan **header** status to `done` (this is the final round).

## Step 13: Robust plan completion + move to `.plan/02-done/`

This is the final round, so the move must be **verified, history-preserving, and reversible on
failure** — not a bare `mv`. Do it as the orchestrator (Claude), never Codex (git is orchestrator-only).

**Preconditions (all must hold before moving — fail loudly if not):**

1. `just check` is green (Step 11).
2. Every Acceptance Criterion below is satisfied — walk the list explicitly.
3. The working tree is committed or the move is staged as part of the round's commit (don't move
   tracked files out from under uncommitted edits).
4. `git grep -n '01-todo/02-spinner-parallel-async-ux'` returns only references that are safe to
   rewrite (the move will break any in-repo links to the old path).

**Procedure:**

```bash
set -euo pipefail
SRC=".plan/01-todo/02-spinner-parallel-async-ux"
DST=".plan/02-done/02-spinner-parallel-async-ux"

# 0. Guard: destination must not already exist (no silent overwrite).
[ -e "$DST" ] && { echo "ERROR: $DST already exists"; exit 1; }

# 1. History-preserving move (use git mv so the rename is tracked).
mkdir -p "$(dirname "$DST")"
git mv "$SRC" "$DST"

# 2. Rewrite any in-repo references to the old location (README index, cross-links).
#    Review each hit first; only rewrite intra-plan/index links, never historical notes.
git grep -lz '01-todo/02-spinner-parallel-async-ux' -- ':!*/02-done/*' \
  | xargs -0 -r sed -i 's#01-todo/02-spinner-parallel-async-ux#02-done/02-spinner-parallel-async-ux#g'

# 3. Verify the move and that no dangling path remains.
[ -d "$DST" ] && [ ! -e "$SRC" ] || { echo "ERROR: move did not complete"; exit 1; }
! git grep -q '01-todo/02-spinner-parallel-async-ux' -- ':!*/02-done/*' \
  || { echo "ERROR: stale references to old plan path remain"; git grep -n '01-todo/02-spinner-parallel-async-ux'; exit 1; }
```

**Post-move:**

- Update the top-level `.plan` README/index (if one tracks todo-vs-done) so the round shows under
  `02-done/`.
- Re-run `just check` once more (the link rewrites touched markdown only, but confirm nothing
  references a moved path in a way the lint catches, e.g. `lychee`).
- Report the final file list and the new plan location.

> Rationale: `git mv` keeps `git log --follow` working on the round files; the destination-exists
> guard prevents clobbering a prior `02-done` entry; the reference sweep prevents dead links in the
> index; the verification steps make the completion self-validating (FIRST applied to the plan
> mechanics themselves).

## Acceptance Criteria

1. **Step 0 — race fixed both directions.** With an expired token + faked token endpoint, an
   `account health` run triggers **exactly one** refresh, the **seed retains a valid rotated refresh
   token** (never orphaned/dead), a follow-up quota fetch succeeds, and there is no false
   `token=invalid` in either race direction. A valid token triggers **zero** refreshes.
2. `doctor` shows a per-check rolling spinner in TTY text mode; hidden in JSON mode and non-TTY.
3. `account refresh` / `account add` show a spinner before and after login, never during the
   interactive child; the spinner is cleared before the child spawns (no ANSI bleed).
4. `SpinnerHandle` cleans up on drop (terminal restored on early return / error), with a unit test.
5. CTRL-C during a spinner op exits without hanging (automated, bounded) and leaves a clean terminal
   (manual TTY check per Step 5); the pass_through exec forwarding path is untouched.
6. All spinner messages follow the §9b spec (Step 10 table).
7. **Test suite is guide-aligned (Step 6):** colocated unit tests for the extracted pure helpers
   (`spinner_policy`, `token_rotated`, `read_access_token`, finish-marker rendering, `Drop`); one
   integration file per new subcommand surface; insta snapshot for doctor output; refresh-race
   regression tests (Step 9) asserting request counts + persisted seed state. Tests are isolated
   (`env_clear` + tempdir), use fakes at boundaries, and pass the import-removal heuristic.
8. New integration tests pass:
   - Doctor piped output clean; doctor JSON valid; doctor snapshot stable.
   - Refresh / add piped mode: no spinner artifacts; success persists state.
   - Health failing-account cleanup; health killed-during-fetch exits cleanly.
   - Refresh-race: refreshes-exactly-once, persists-rotation-to-seed, quota-then-succeeds,
     live-token-no-refresh.
9. All existing tests still pass (`just test`).
10. `just test-unit`, `just test-integration`, `just lint`, and `just check` all pass; no new
    warnings, no global `#[allow]` shims.
11. Plan completed and moved to `.plan/02-done/02-spinner-parallel-async-ux/` via the robust Step 13
    procedure (history-preserving, no dangling references, verified).

## Next Round

Final round. No further rounds.

> Recommended follow-ups (not gated here): add `proptest` for the rotation/round-trip invariants and
> run `cargo-mutants` on the Step 0 refresh coordinator + `read_access_token` (target ≥ 60% mutants
> killed) — see Step 6.
