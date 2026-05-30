# Spinner UX + Parallel Async Operations

> Complexity: L | Rounds: 3 | Generated: 2026-05-27 | Repo: /workspaces/codex-session | Status: todo

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

The work splits into three rounds following dependency order:

1. **Round 01 — Async Runtime Migration**: Add tokio + indicatif dependencies, switch reqwest from
   blocking to async, migrate main.rs/dispatch/commands/services to async. All existing tests must
   still pass. No UX changes — this is pure infrastructure.

2. **Round 02 — Spinner Module + Account Health & Quota Parallelism**: Create `src/ui/spinner.rs`
   with reusable spinner helpers built on indicatif's `MultiProgress`. Redesign account health to
   run all accounts in parallel (tokio::JoinSet) with per-account spinners, and within each account
   run quota fetch + heartbeat probe concurrently. Same treatment for account quota. Add live tests
   with wiremock to verify parallelism and output correctness.

3. **Round 03 — Doctor, Refresh, Add Spinners + Polish**: Add step-by-step progress to doctor,
   spinners to account refresh/add login flows, and comprehensive edge-case handling. Integration
   tests for TTY/non-TTY behavior, --format json suppression, and signal cleanup.

## Execution Order

| Round | File                                  | Topic                                    | Status | Completed |
| ----- | ------------------------------------- | ---------------------------------------- | ------ | --------- |
| 01    | `01-async-runtime-migration.md`       | Tokio + async reqwest migration          | todo   | --        |
| 02    | `02-spinner-parallel-health-quota.md` | Spinner module + health/quota parallel   | todo   | --        |
| 03    | `03-doctor-refresh-add-polish.md`     | Doctor/refresh/add spinners + edge cases | todo   | --        |

## Execution Commands

```bash
# Execute a single round:
/prex -ar .plan/01-todo/spinner-parallel-async-ux/01-async-runtime-migration.md
/prex -ar .plan/01-todo/spinner-parallel-async-ux/02-spinner-parallel-health-quota.md
/prex -ar .plan/01-todo/spinner-parallel-async-ux/03-doctor-refresh-add-polish.md

# Execute with full directory context:
/prex -ar @.plan/01-todo/spinner-parallel-async-ux/
```

## Decisions & Constraints

1. **Tokio as the async runtime.** The project already uses tokio in dev-dependencies (for wiremock
   tests). Moving it to production deps unifies the async story. Reqwest's default features enable
   async; the `blocking` feature is removed.

2. **indicatif for spinners.** 5.1K GitHub stars, 136M crates.io downloads, `MultiProgress` for
   concurrent spinners, thread-safe (`Sync + Send`), steady-tick for automatic animation, and
   first-class tokio compatibility. It's the de facto standard for Rust CLI progress indication.

3. **Spinner module lives in `src/ui/spinner.rs`.** Complements the existing `src/ui/mod.rs` output
   module and the in-progress CLI design system plan. Spinners share the color detection
   infrastructure from `src/ui/color.rs`.

4. **Two levels of parallelism in account health.** Across accounts: `tokio::JoinSet` runs all
   account checks concurrently. Within each account: `tokio::join!` runs quota fetch and heartbeat
   probe concurrently. Maximum speedup for multi-account setups.

5. **Spinners suppressed in non-TTY and --format json.** When stdout is piped or output format is
   JSON, spinners are silently disabled (indicatif's `ProgressDrawTarget::hidden()`). Machine-
   readable output must never contain spinner artifacts.

6. **gate.rs heartbeat probe migrates to `tokio::process::Command`.** The probe spawns a child
   process and polls `try_wait()` in a loop with `thread::sleep(200ms)`. This becomes
   `tokio::process::Command` with `.wait_with_output()` + `tokio::time::timeout()`.

7. **All existing tests must pass after Round 01.** The async migration is a refactor — behavior is
   identical. Integration tests use `assert_cmd` (subprocess-based) so they are unaffected by the
   internal async change.

8. **Live tests use wiremock for HTTP mocking.** The project already has this pattern (see
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

- **Async migration cascade:** Making `dispatch::run()` async forces every command handler to become
  async. This is mechanical (add `async` keyword + `.await` at call sites) but touches many files.
  Round 01 is carefully scoped to change only signatures and awaits, not behavior.

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

- **Design system plan overlap.** The existing `.plan/01-todo/cli-design-system-colorful-output/`
  plan defines styling conventions. The spinner module should follow those conventions (colors,
  symbols) but is otherwise independent. No file conflicts — the design system plan touches
  `ui/mod.rs` rendering functions, while this plan adds `ui/spinner.rs`.

## Completion

When all rounds are done:

```bash
# Update status in this file to "done"
# Fill in completion timestamps in the execution order table
mkdir -p .plan/02-done && mv .plan/01-todo/spinner-parallel-async-ux .plan/02-done/spinner-parallel-async-ux
```
