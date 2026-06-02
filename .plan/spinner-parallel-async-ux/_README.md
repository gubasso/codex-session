# Spinner UX + Parallel Async Operations

> Complexity: L | Rounds: 4 | Generated: 2026-05-27 | Repo: /workspaces/codex-session | Status: done

## Problem Statement

codex-session's CLI commands that make network requests (account health, account quota) run
sequentially with no user feedback during wait times. When checking multiple accounts, the user
stares at a blank terminal for 10-30+ seconds (10s quota timeout + 15s probe timeout per account,
all sequential). There are no spinners, no progress indication, and no parallelism.

The project currently uses `reqwest::blocking` with no async runtime in production code. Network
calls happen one-at-a-time in `for` loops. The `account health` command is the worst offender: for
N accounts it runs N sequential quota fetches (10s timeout each) + N sequential heartbeat probes
(15s timeout each), meaning 2 accounts can block for up to 50 seconds.

This plan adds:

1. **Tokio async runtime** — migrates from blocking reqwest to async, enabling parallel I/O
2. **indicatif spinners** — beautiful terminal spinners during all wait-time operations
3. **Parallel execution** — concurrent account checks with multi-spinner feedback
4. **Live integration tests** — wiremock-based tests verifying parallelism and spinner suppression

## Strategy

The original work split into **three rounds** following dependency order, with a fourth round
appended afterward as a deferred follow-up. Sizing was measured with the plan-writer complexity
heuristic for the `/prex` executor (Executor Factor 1.5), which favours fewer, larger, cohesive
rounds because its in-round review-loop absorbs complexity. The style-guide spinner spec (a
governance prerequisite per CLAUDE.md) is folded into Round 02 as its first step rather than being a
standalone undersized round; the async migration (Round 01) changes no runtime narration, so it
needs no spec.

1. **Round 01 — Async Runtime Migration** (M): Add tokio + indicatif dependencies, switch reqwest
   from blocking to async, and make async **only the network surface** — `main`, `dispatch::run`, the
   `account health` / `account quota` handlers, and the quota service. `token_refresh` stays blocking
   (invoked via `spawn_blocking`); `retry.rs`, `pass_through`, `run_child`, and the signal-forwarding
   spawner stay **synchronous** (the `codex` exec hot path is a blocking, stdio-inherited child wait
   that gains nothing from async). All existing tests must still pass. No UX changes.

2. **Round 02 — Style-Guide Spec + Spinner Module + Health & Quota Parallelism** (M, heaviest):
   First amend `docs/design/cli-style-guide.md` with the §9b spinner/progress spec (governance gate).
   Then create `src/ui/spinner.rs` (indicatif `MultiProgress`) implementing it, bridge `tracing` so
   it doesn't corrupt frames, and redesign account health + account quota to run all accounts in
   parallel (`tokio::JoinSet`) with per-account spinners — within each health account, quota fetch +
   heartbeat probe run concurrently (`tokio::join!`). Re-sort results for deterministic output. Add
   wiremock live tests (concurrency-based, not flaky wall-clock).

3. **Round 03 — Doctor/Refresh/Add Spinners + Refresh-Race Fix + Robust Tests** (L): First fix the
   remaining quota/probe single-use-refresh-token race (the inverse / probe-wins case carried over
   from Round 02 review — Step 0). Then add rolling progress to doctor, before/after-login spinners
   to account refresh/add, `Drop`-based cleanup, the concrete signal expectation, and the
   message-consistency pass. Replace the draft test sketches with a guide-aligned pyramid (colocated
   unit tests + per-subcommand integration + insta snapshots + refresh-race regression tests) per
   `cli-design/08-testing-and-quality`. Finish with a robust, history-preserving move to
   `.plan/02-done/`.

4. **Round 04 — Health Probe vs. Quota Auth-Source Authority** (deferred follow-up): Close the
   stale-seed/live-group false-negative in `account health` — the probe reads the account **seed**
   while quota resolves a newer per-group `auth.json`, so a healthy account with a stale seed can be
   reported `token=invalid`. Surfaced by the Round 03 review-loop and explicitly scoped out of Round
   03 (auth-authority redesign); tracked here so it stays with the plan that found it. Pre-existing,
   not a Round 03 regression.

## Execution Order

| Round | File                                       | Topic                                                             | Status | Completed  |
| ----- | ------------------------------------------ | ----------------------------------------------------------------- | ------ | ---------- |
| 01    | `01-async-runtime-migration.md`            | Tokio + async reqwest migration (network surface)                 | done   | 2026-06-01 |
| 02    | `02-spinner-parallel-health-quota.md`      | §9b spec + spinner module + health/quota parallel                 | done   | 2026-06-01 |
| 03    | `03-doctor-refresh-add-polish.md`          | Doctor/refresh/add spinners + refresh-race fix + robust tests     | done   | 2026-06-01 |
| 04    | `04-health-probe-auth-source-authority.md` | Health probe vs. quota auth-source authority (deferred follow-up) | done   | 2026-06-01 |

## Execution Commands

```bash
# Execute a single round (note the 02- directory prefix):
/prex -ar .plan/01-todo/02-spinner-parallel-async-ux/01-async-runtime-migration.md
/prex -ar .plan/01-todo/02-spinner-parallel-async-ux/02-spinner-parallel-health-quota.md
/prex -ar .plan/01-todo/02-spinner-parallel-async-ux/03-doctor-refresh-add-polish.md
/prex -ar .plan/01-todo/02-spinner-parallel-async-ux/04-health-probe-auth-source-authority.md

# Execute with full directory context:
/prex -ar @.plan/01-todo/02-spinner-parallel-async-ux/
```

## Decisions & Constraints

1. **Tokio as the async runtime.** The project already uses tokio in dev-dependencies (for wiremock
   tests). Moving it to production deps unifies the async story. Reqwest's default features enable
   async; the `blocking` feature is removed from the quota client. **`token_refresh.rs` keeps using
   `reqwest::blocking`** and is reached from async code only via `tokio::task::spawn_blocking` — see
   decision 9.

2. **Async blast radius is confined to the network surface (plan-review decision).** Making
   `token_refresh::refresh_token` async would cascade into `retry.rs` (`try_refresh`, `run_auto`,
   `single_attempt`) and the entire `pass_through` → `run_child` → signal-forwarding exec path,
   which is a synchronous, stdio-inherited blocking child wait that gains nothing from async and is
   high-risk to convert. Instead: keep `token_refresh`, `retry.rs`, `pass_through`, and
   `adapters/spawner.rs` **synchronous**. Only `account health`, `account quota`, and the quota
   service become truly async; `token_refresh` is called from the async quota service via
   `spawn_blocking`. `dispatch::run` is async but routes the External/None (pass_through) arm to the
   existing sync path (sync is callable from async).

3. **Style guide is amended before any spinner code (plan-review decision).** CLAUDE.md makes
   `docs/design/cli-style-guide.md` the source of truth for runtime narration; it currently defines
   no spinner spec (§9 is quota progress bars only). The §9b spec is added as **Round 02 Step 0**
   (its first step, before the module), so the spinner module implements it rather than inventing
   conventions. It is not a standalone round because, measured against the complexity heuristic, a
   doc-only round is undersized (S, raw 5) and the spec is cohesive with the module that implements
   it.

4. **Executor: `prex` (EF 1.5); 3 rounds (plan-review sizing decision).** Rounds were sized with the
   plan-writer complexity heuristic. The whole plan is L (override from adjusted-M because of the
   hard async⇒infra⇒consumers dependency spine and the 600s/round ceiling). Under `prex` the
   in-round review-loop favours fewer, larger, cohesive rounds, so health+quota parallelism share
   Round 02 (its heaviest round, adj ≈ 10.0 M). If a future re-plan targets a `limited` executor
   (EF 0.8), split Round 02 into separate health and quota rounds.

5. **indicatif for spinners.** 5.1K GitHub stars, 136M crates.io downloads, `MultiProgress` for
   concurrent spinners, thread-safe (`Sync + Send`), steady-tick for automatic animation, and
   first-class tokio compatibility. It's the de facto standard for Rust CLI progress indication.

6. **Spinner module lives in `src/ui/spinner.rs`.** Complements the existing `src/ui/mod.rs` output
   module and the in-progress CLI design system plan. Spinners share the color detection
   infrastructure from `src/ui/color.rs`.

7. **Two levels of parallelism in account health.** Across accounts: `tokio::JoinSet` runs all
   account checks concurrently. Within each account: `tokio::join!` runs quota fetch and heartbeat
   probe concurrently. Maximum speedup for multi-account setups.

8. **Spinners suppressed in non-TTY and --format json.** When stdout is piped or output format is
   JSON, spinners are silently disabled (indicatif's `ProgressDrawTarget::hidden()`). Machine-
   readable output must never contain spinner artifacts.

9. **gate.rs heartbeat probe migrates to `tokio::process::Command`.** The probe spawns a child
   process and polls `try_wait()` in a loop with `thread::sleep(200ms)`. This becomes
   `tokio::process::Command` with `.wait_with_output()` + `tokio::time::timeout()`.

10. **All existing tests must pass after Round 01.** The async migration is a refactor — behavior is
    identical. Integration tests use `assert_cmd` (subprocess-based) so they are unaffected by the
    internal async change.

11. **Live tests use wiremock for HTTP mocking.** The project already has this pattern (see
    `tests/account_quota_http.rs`). New tests verify parallel fetch timing, spinner suppression in
    piped output, and multi-account correctness.

## Rejected Alternatives

- **`std::thread::scope` for parallelism (stay blocking):** Simpler but prevents future async
  benefits (streaming responses, backpressure, cancellation). Since tokio is already in
  dev-dependencies and wiremock requires it, adding it to production is a small incremental cost.

- **`spinners` or `spinoff` crate:** Lack `MultiProgress` for concurrent spinners. `spinners` has
  no multi-spinner support at all. indicatif is the only crate with first-class concurrent spinner
  management.

- **`rayon` for parallelism:** Designed for CPU-bound work. The bottleneck here is I/O (network
  requests, child process waits), which is tokio's sweet spot.

- **Custom spinner implementation:** The `anstyle` infrastructure could support basic cursor
  manipulation, but reinventing `MultiProgress`, steady-tick timing, and terminal state cleanup is
  not worth it when indicatif exists.

## Risks & Edge Cases

- **Async migration cascade (RESOLVED by decision 9):** The original plan claimed making
  `dispatch::run()` async forces _every_ handler async "with no other changes." That is false — it
  would drag in `retry.rs` and the `pass_through` exec/signal path. Round 01 is now scoped to make
  async only the network surface (health/quota + quota service), with `token_refresh` reached via
  `spawn_blocking` and the exec path left synchronous.

- **`tracing` shares stderr with spinners.** `src/logging.rs` writes the fmt layer to
  `std::io::stderr`. Network ops emit `tracing` events while spinners animate on the same stream, so
  spinner output must be guarded (route logs through `ProgressBar::suspend` / a MultiProgress-aware
  writer, or gate spinners to the default quiet log level). Addressed in Round 02.

- **Heartbeat probe spawns a child process.** `tokio::process::Command` should work identically to
  `std::process::Command` but needs testing — the probe uses env isolation, stdin/stdout piping, and
  timeout-based kill. The live test in Round 01 must verify this.

- **Spinner output on stderr.** indicatif writes to stderr by default, which is correct (command
  output goes to stdout). But if the wrapper's `write_warning()` also writes to stderr, there could
  be interleaving. The spinner module must use indicatif's `suspend()` method when writing non-
  spinner stderr output.

- **CTRL-C during spinners.** indicatif handles terminal restoration on drop, but if the process is
  killed with SIGKILL, the terminal may be left in a bad state. The project already has signal
  handling (`signal-hook` crate). Spinners should be dropped cleanly in signal handlers.

- **Design system overlap is real on this branch (RESOLVED by decision 10).** This work lands on the
  CLI-design-system branch, and `docs/design/cli-style-guide.md` is the governing source of truth for
  runtime narration but defines **no** spinner spec (§9 is quota progress bars only, explicitly).
  Round 02 Step 0 amends the guide first; the spinner module then reuses §5 symbols (`✓`/`✗`,
  `[ok]`/`[err]` fallback) and §7 status colors and emits to stderr per §2.

## Completion

When all rounds are done:

```bash
# Update status in this file to "done"
# Fill in completion timestamps in the execution order table
mkdir -p .plan/02-done && mv .plan/01-todo/02-spinner-parallel-async-ux .plan/02-done/02-spinner-parallel-async-ux
```
