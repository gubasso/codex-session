# Round 02 — Style-Guide Spinner Spec + Spinner Module + Parallel Account Health & Quota

> Round 2 of 3 | Topic: style-guide spinner spec + indicatif spinner helpers + parallel account operations | Status: todo

## Context

codex-session is a Rust CLI wrapper that manages multi-account credential pooling for the `codex`
binary. Commands like `account health` and `account quota` fetch data from remote APIs sequentially
per account with no progress feedback. After Round 01, the codebase uses tokio + async reqwest,
making parallel I/O and spinner integration straightforward.

This round first amends the CLI style guide with a spinner / progress-narration spec (governance:
`CLAUDE.md` makes `docs/design/cli-style-guide.md` the source of truth for runtime narration, and it
currently defines no spinner spec — §9 covers only quota progress bars), then creates a reusable
spinner module built on `indicatif` that _implements_ that spec, and redesigns `account health` and
`account quota` to run all account operations in parallel with per-account spinner feedback.

> **Plan-review prerequisites for this round.** Step 0 below writes the style-guide spec; the spinner
> module (Step 1) must _implement_ it (symbols, colors, suppression matrix), not invent it. Three
> correctness items from review apply throughout: (a) `tracing` co-tenants stderr with spinners and
> must be suspended (MAJOR 3); (b) `JoinSet` completion order is nondeterministic, so results must be
> re-sorted before rendering (MINOR 7/8); (c) the new tests must use **real** helpers — the draft
> references several that don't exist (MAJOR 4).

## Current State

After Round 01, the following is true:

- Tokio runtime is active (`#[tokio::main]` in main.rs)
- `dispatch::run` is async; **only `account health` and `account quota` handlers + the quota service
  are `async fn`** — all other handlers stay sync (BLOCKER 1 scope)
- `reqwest::Client` (async) replaces `reqwest::blocking::Client` **in the quota service only**;
  `token_refresh.rs` keeps `reqwest::blocking` and is called via `spawn_blocking`
- `tokio::process::Command` replaces `std::process::Command` for the gate.rs heartbeat probe;
  `gate::run_login` stays synchronous
- `retry.rs`, `pass_through.rs`, `run_child`, `adapters/spawner.rs` are unchanged (synchronous)
- `indicatif = "0.17"` is in Cargo.toml but not yet used

### account health command (sequential, no spinners)

File: `/workspaces/codex-session/src/commands/account/health.rs`

```rust
// After Round 01 this is async but still sequential:
for entry in accounts {
    entries.push(build_entry(&BuildEntryInput {
        ctx, registry: &registry, entry: &entry,
        active: active.as_ref(), fast: args.fast, now,
    }).await);
}
```

The `build_entry()` function calls:

- `fetch_quota()` → `quota::refresh().await` (10s HTTP timeout)
- `fetch_probe()` → `gate::probe_token().await` (15s process timeout)

These two are independent and can run concurrently within each account.

### account quota command (sequential, no spinners)

File: `/workspaces/codex-session/src/commands/account/quota.rs`

```rust
// After Round 01 this is async but still sequential:
for entry in registry.list()? {
    entries.push(fetch_view(ctx, &entry.id, ...).await?);
}
```

### UI module (no spinner infrastructure)

File: `/workspaces/codex-session/src/ui/mod.rs` — has `Ui` struct with `write_*` methods,
`color.rs` for ANSI detection, `anstyle`-based styling.

File: `/workspaces/codex-session/src/ui/color.rs` — provides:

```rust
pub(crate) fn should_color(stream: Stream) -> bool
```

### Existing test pattern for HTTP mocking

File: `/workspaces/codex-session/tests/account_quota_http.rs` — uses `wiremock::MockServer`,
`#[tokio::test]`, and `assert_cmd` to run the binary as subprocess with
`CODEX_SESSION_WHAM_USAGE_URL` pointed at the mock server.

File: `/workspaces/codex-session/tests/support/mod.rs` — `TestEnv` struct with `cmd()` helper that
creates a hermetic `assert_cmd::Command` with env isolation.

## Previous Rounds

**Round 01** migrated the **network surface** from blocking to async (BLOCKER 1 scope):

- Added tokio + indicatif to Cargo.toml (kept reqwest `blocking` for token_refresh)
- Switched the quota service's reqwest client to async
- Made `dispatch::run` + `account health`/`account quota` handlers + quota service async; left all
  other handlers, `retry.rs`, `pass_through`, and `gate::run_login` synchronous
- All tests pass, behavior unchanged

## Scope of This Round

### In scope

1. Amend `docs/design/cli-style-guide.md` with a spinner / progress-narration spec (§9b)
2. Create `src/ui/spinner.rs` module with reusable spinner helpers implementing that spec
3. Redesign `account health` for parallel execution with multi-spinner
4. Redesign `account quota` for parallel execution with multi-spinner
5. Suppress spinners when `--format json` or non-TTY (piped output)
6. Live integration tests for parallelism correctness and output integrity

### Out of scope

- Doctor progress indication (Round 03)
- Account refresh/add spinners (Round 03)
- Error path spinners (Round 03)

## Implementation Steps

### Step 0: Amend the CLI style guide with a spinner spec (governance — do this first)

File: `/workspaces/codex-session/docs/design/cli-style-guide.md`

`CLAUDE.md` makes this guide the source of truth for runtime narration; it must define spinners
before code introduces them. Today: §2 routes progress narration to **stderr** (reuse); §5 defines
`✓`/`✗`/`▸`/`—` symbols; §7 defines status colors; §9 says "Progress bars are used only for quota
percentage displays" and covers no spinners.

Add a subsection **§9b "Spinners & live progress narration"** defining, at minimum:

1. **When spinners appear** — `account health`, `account quota`, `doctor` (rolling single-line),
   `account refresh` / `account add` (before/after the interactive login, never _during_ it).
2. **Channel** — stderr only (consistent with §2). Command results stay on stdout.
3. **Suppression matrix** — hidden when **any** of: stderr is not a TTY; `--format json`;
   `--quiet` / `--silent` (§11); `account health --fast`.
4. **Frame & color** — `{spinner:.cyan} {msg}`, `enable_steady_tick(80ms)`; glyph color reconciled
   with §7; gated on `color::should_color(Stream::Stderr)`; `NO_COLOR`/non-TTY ⇒ plain.
5. **Finish markers** — `✓ <msg>` (GREEN) / `✗ <msg>` (RED), with `[ok]` / `[err]` ASCII fallback
   when color is off; transient spinners finish-and-clear (no residual line).
6. **Coexistence with `tracing`** — while a spinner is live, other stderr writers (`write_warning`
   and `tracing` events) must go through the spinner suspend mechanism so frames aren't corrupted.
7. **Message conventions** — present participle for in-progress, account names quoted, finish
   messages ≤ 60 chars. (This is the canonical source for the message table in Round 03 Step 10.)

Also adjust the §9 "only for quota percentage displays" sentence so it scopes the _bar_ widget, not
animated spinners. Update any section index/TOC.

> No code in this step — just the guide. The spinner module (Step 1) implements §9b.

### Step 1: Create src/ui/spinner.rs module

File: `/workspaces/codex-session/src/ui/spinner.rs` (new file)

Create a spinner helper module that wraps `indicatif` and integrates with the project's
color/TTY detection.

Key components:

```rust
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

/// Controls whether spinners are visible (TTY + text format) or hidden (piped / JSON).
pub(crate) struct SpinnerGroup {
    multi: MultiProgress,
}

impl SpinnerGroup {
    /// Create a new spinner group. If `visible` is false, all spinners draw to a hidden target
    /// (no terminal output, no overhead).
    pub(crate) fn new(visible: bool) -> Self { ... }

    /// Add a new spinner with a message. Returns a handle that auto-finishes on drop.
    pub(crate) fn add(&self, message: &str) -> SpinnerHandle { ... }
}

pub(crate) struct SpinnerHandle {
    bar: ProgressBar,
}

impl SpinnerHandle {
    /// Update the spinner message.
    pub(crate) fn set_message(&self, msg: impl Into<std::borrow::Cow<'static, str>>) { ... }

    /// Mark this spinner as successfully completed. Changes to a checkmark.
    pub(crate) fn finish_ok(&self, msg: &str) { ... }

    /// Mark this spinner as failed. Changes to a cross.
    pub(crate) fn finish_err(&self, msg: &str) { ... }

    /// Finish and clear the spinner line (no residual output).
    pub(crate) fn finish_and_clear(&self) { ... }
}
```

Design choices:

- Use `ProgressStyle::with_template("{spinner:.cyan} {msg}")` for the default spinner style
- Use `enable_steady_tick(Duration::from_millis(80))` for smooth animation
- The visibility decision is made once at group creation based on `color::should_color()` and the
  output format. When hidden, `ProgressDrawTarget::hidden()` eliminates all overhead.
- `finish_ok` renders as `✓ message` (green), `finish_err` renders as `✗ message` (red)
- Symbols and colors should respect `NO_COLOR` / non-TTY — when color is off, use plain text
  markers like `[ok]` and `[err]` instead of Unicode symbols

### Step 2: Register spinner module in ui/mod.rs

File: `/workspaces/codex-session/src/ui/mod.rs`

Add `pub(crate) mod spinner;` to the module declarations (after `pub(crate) mod color;`).

### Step 3: Redesign account health for parallel execution

File: `/workspaces/codex-session/src/commands/account/health.rs`

Replace the sequential `for` loop (line 53) with parallel execution using `tokio::JoinSet`:

```rust
use tokio::task::JoinSet;
use crate::ui::spinner::SpinnerGroup;

pub(crate) async fn run(ctx: &crate::context::AppContext, args: AccountHealthArgs) -> Result<(), AppError> {
    // ... existing validation and account resolution ...

    let show_spinner = !args.fast
        && args.format == OutputFormat::Text
        && std::io::stderr().is_terminal();
    let spinners = SpinnerGroup::new(show_spinner);

    let mut set = JoinSet::new();
    for entry in accounts {
        let spinner = spinners.add(&format!("Checking account \"{}\"...", entry.id));
        // Clone/Arc what's needed for the spawned task
        set.spawn(async move {
            let result = build_entry(&input).await;
            match &result {
                // Update spinner with success/failure
                _ if result.status == "live" => spinner.finish_ok(&format!("{} done", entry.id)),
                _ => spinner.finish_err(&format!("{} {}", entry.id, result.status)),
            }
            result
        });
    }

    let mut entries = Vec::new();
    while let Some(result) = set.join_next().await {
        entries.push(result?); // result? handles JoinError (task panic) only — see note
    }

    // ... existing sort, rank, render logic ...
}
```

> **MINOR 7 — `build_entry` is infallible.** In the current code (`health.rs:94`) `build_entry`
> returns `AccountHealthEntryView` directly; failures are encoded as `status` strings
> ("fetch failed", "cache missing"). So each spawned task returns a `View`, and `result?` in
> `join_next` only unwraps a `JoinError` (panic), not a domain error. Keep it that way.
>
> **Must preserve from the current `run()`** (the pseudo-code above omits these): the `--account auto`
> rejection (`health.rs:21-26`); the `validate_ping_config_recipe` + `ensure_child_version` guard for
> non-fast mode (`28-31`); the single-named-account selection branch (`38-52`); and the
> **post-collection sort + rank assignment** (`65-78`). Since `JoinSet::join_next` yields in
> completion order, that existing sort is exactly what restores deterministic output ordering — do
> not remove it.
>
> **Send/`'static`:** `AppContext` is not `Clone` today but its fields are `Send + Sync`
> (`Arc<Config>`, ZST `Ui`/`StdSpawner`, `Utf8PathBuf`, `OnceLock`-based lazies). Wrap it once in
> `Arc<AppContext>` before the loop and clone the `Arc` into each task. `AccountEntry` is
> `#[derive(Clone)]`; `Registry` is cheaply rebuilt from `Arc<Config>` inside the task if needed.

**Important**: `build_entry()` references `ctx` (which is `&AppContext`) and `registry` (which is
`&Registry`). These are not `Send` if they contain non-Send fields. There are two approaches:

a. Wrap `AppContext` in `Arc` and make it `Send + Sync` (if it already is — check the types)
b. Extract the data needed by each task into a `Send`-safe struct before spawning

Check whether `AppContext`, `Registry`, `AccountEntry` are `Send + Sync`. If not, extract the
needed fields (config paths, auth paths, etc.) into a plain data struct that is `Send`.

Alternatively, use `tokio::task::spawn_blocking()` if the async overhead is not worth it for these
operations, but this defeats the purpose of async. Prefer making the data `Send`.

### Step 4: Inner parallelism in build_entry (quota + probe concurrent)

File: `/workspaces/codex-session/src/commands/account/health.rs`

Within `build_entry()`, the quota fetch and heartbeat probe are independent. Run them concurrently:

```rust
async fn build_entry(input: &BuildEntryInput<'_>) -> AccountHealthEntryView {
    // ... token_state, plan_bonus, cooldown reads (local, fast) ...

    // Run quota fetch and probe in parallel
    let (quota_result_tuple, probe) = tokio::join!(
        fetch_quota(ctx, account, fast),
        fetch_probe(ctx, account, fast),
    );
    let (quota_result, fetched_at_unix, status) = quota_result_tuple;

    // ... scoring, view construction (unchanged) ...
}
```

This means for N accounts, the total wall time is approximately:
`max(max_quota_time, max_probe_time)` instead of `sum(quota_time) + sum(probe_time)`.

### Step 5: Redesign account quota for parallel execution

File: `/workspaces/codex-session/src/commands/account/quota.rs`

Same pattern as health — replace the sequential `for` loop (line 52) with `JoinSet`:

```rust
pub(crate) async fn run(ctx: &AppContext, args: AccountQuotaArgs) -> Result<(), AppError> {
    // ... existing setup ...

    if let Some(ref target) = single_account {
        // Single account: no parallelism needed, but still show spinner
        let show_spinner = args.format == OutputFormat::Text && std::io::stderr().is_terminal();
        let spinners = SpinnerGroup::new(show_spinner);
        let spinner = spinners.add(&format!("Fetching quota for \"{}\"...", target));
        let result = fetch_view(ctx, target, ...).await;
        spinner.finish_and_clear();
        entries.push(result?);
    } else {
        // Multiple accounts: parallel fetch with multi-spinner
        let show_spinner = args.format == OutputFormat::Text && std::io::stderr().is_terminal();
        let spinners = SpinnerGroup::new(show_spinner);

        let mut set = JoinSet::new();
        for entry in registry.list()? {
            let spinner = spinners.add(&format!("Fetching quota for \"{}\"...", entry.id));
            set.spawn(async move {
                let result = fetch_view(ctx, &entry.id, ...).await;
                match &result {
                    Ok(_) => spinner.finish_ok(&format!("{} done", entry.id)),
                    Err(_) => spinner.finish_err(&format!("{} failed", entry.id)),
                }
                result
            });
        }

        while let Some(result) = set.join_next().await {
            match result? {
                Ok(view) => entries.push(view),
                Err(err) if !single_account => {
                    // Multi-mode: push error view instead of propagating
                    entries.push(error_view(...));
                }
                Err(err) => return Err(err.into()),
            }
        }

        // MINOR 8 — re-sort before rendering. JoinSet yields in completion order, but the
        // current command emits in registry.list() order. Sort by a stable key (account id)
        // so output is deterministic and existing snapshot/order tests don't flake:
        entries.sort_by(|a, b| a.account.cmp(&b.account));
    }

    // ... existing render logic ...
}
```

> Preserve the current single-account-propagates vs multi-account-error-view policy exactly (see
> service `quota.rs` multi-branch). Only the iteration becomes parallel; the error policy is
> unchanged.

### Step 6: Add spinner suppression logic

The spinner visibility decision implements the §9b suppression matrix (Step 0):

1. **Output format**: When `--format json`, spinners are hidden (JSON output must be clean)
2. **TTY detection**: When stderr is not a terminal (piped), spinners are hidden
3. **`--quiet` / `--silent`**: per style guide §11 these suppress non-error stderr → hide spinners
4. **`--fast` flag** (health only): When fast mode, no network calls → no spinners

Create a helper in `spinner.rs` (reuse `ui::color::should_color(Stream::Stderr)` so the `NO_COLOR` /
`FORCE_COLOR` / TTY logic stays in one place rather than re-checking `is_terminal()` directly):

```rust
pub(crate) fn should_show_spinner(format: crate::cli::OutputFormat /*, quiet/silent flags */) -> bool {
    matches!(format, crate::cli::OutputFormat::Text)
        && std::io::stderr().is_terminal()
        // && !quiet && !silent
}
```

Command handlers call this to decide the `visible` parameter for `SpinnerGroup::new()`. Confirm the
exact `--quiet`/`--silent` flag names against `src/cli/` before wiring them.

### Step 7: Handle indicatif + stderr interleaving (warnings AND tracing)

Two writers share stderr with the spinners:

1. `ctx.ui.write_warning()` / `write_prompt()` — wrap these in `MultiProgress::suspend()`.
2. **`tracing` (MAJOR 3).** `src/logging.rs:134,143` configures the `tracing_subscriber` fmt layer
   with `.with_writer(std::io::stderr)`. The async network ops emit `tracing::info!`/`debug!` events
   (e.g. `quota.fetch`, `quota.token_refresh`) **while spinners animate on the same stream**. Plain
   `suspend()` around `write_warning` does NOT cover these — they fire from inside the spawned tasks.

```rust
impl SpinnerGroup {
    /// Suspend spinner rendering, execute a closure, then resume.
    pub(crate) fn suspend<F, R>(&self, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        self.multi.suspend(f)
    }
}
```

Pick one approach for tracing and document it in the round:

- **(a) Bridge the log writer through the active progress draw target** — e.g. the
  `indicatif_log_bridge` crate, or a custom `MakeWriter` that calls `ProgressBar::suspend`. Cleanest;
  works at any log level.
- **(b) Gate spinners to the default quiet log level** — only construct a _visible_ `SpinnerGroup`
  when the stderr filter is at its default (no `RUST_LOG`/`-v` raising it above warn). At higher
  verbosity, fall back to hidden spinners so logs render cleanly.

Whichever is chosen, the integration tests in Steps 8–11 assert no ANSI/frame artifacts in stderr,
which catches regressions here.

### Step 8a (prerequisite): Hoist shared test helpers (MAJOR 4)

The draft test code below references helpers that **do not exist** or are **file-local**. Before
writing new test files, fix the support layer:

| Referenced                                                 | Reality                                                           | Action                                                                   |
| ---------------------------------------------------------- | ----------------------------------------------------------------- | ------------------------------------------------------------------------ |
| `TestEnv::new_with_ping_profile()`                         | does not exist                                                    | add it, or reuse `TestEnv::new()`/`new_empty()` + ping config seed       |
| `TestEnv::cmd_raw()`                                       | does not exist; it's `std_cmd()` (`tests/support/mod.rs:154`)     | use `std_cmd()`                                                          |
| `oauth_auth()`, `payload()`, `wham_url()`, `add_account()` | file-local to `tests/account_quota_http.rs:15-34`                 | hoist into `tests/support/mod.rs` (or a shared `tests/support/quota.rs`) |
| `TEST_AUTH`                                                | file-local to `tests/account_health_cli.rs:11`                    | hoist into `tests/support`                                               |
| delayed-response mock                                      | `.set_delay(Duration)` is valid on wiremock 0.6 but unused so far | add a small `delayed_quota_mock(server, delay)` helper                   |

Only after these exist should the new test files reference them.

### Step 8: Live integration test — parallel health with wiremock

File: `/workspaces/codex-session/tests/account_health_parallel.rs` (new file)

Test that multi-account health runs all accounts concurrently by using wiremock with deliberate
delays (using the hoisted helpers from Step 8a):

```rust
#[tokio::test]
async fn health_multi_account_runs_in_parallel() {
    let env = TestEnv::new_empty(); // or new_with_ping_profile() once added in Step 8a
    env.seed_account("acct1", oauth_auth());
    env.seed_account("acct2", oauth_auth());

    let server = MockServer::start().await;

    // Each quota response takes 2 seconds
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(payload(), "application/json")
                .set_delay(Duration::from_secs(2)),
        )
        .mount(&server)
        .await;

    let start = Instant::now();
    let output = env.cmd()
        .env("CODEX_SESSION_WHAM_USAGE_URL", wham_url(&server))
        .args(["account", "health", "--fast", "--format", "json"])
        .timeout(Duration::from_secs(10))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let elapsed = start.elapsed();

    // If sequential: 2s * 2 accounts = 4s minimum
    // If parallel: ~2s (both run concurrently)
    // Use --fast to skip probe, testing only quota parallelism
    // Note: --fast uses cache, so this test needs a variant without --fast
    // that actually hits the mock server

    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let arr = value.as_array().unwrap();
    assert_eq!(arr.len(), 2);
}
```

> **MINOR 9 — this `--fast` test is internally contradictory and must be split.** `--fast` reads the
> cache and does NOT hit the mock, so the timing comment ("2s × 2 accounts") is meaningless here.
> Resolve it cleanly:
>
> - Use **`account health --fast`** only for _correctness / no-artifact_ assertions (all accounts
>   present, valid JSON, clean stderr) — no timing claim, no mock delay.
> - Do the **timing / parallelism** assertion with **`account quota`** (Step 9), which always hits
>   the network and needs no `codex` stub binary.
>
> So this test should drop the `Instant`/`elapsed` machinery entirely and just assert
> `arr.len() == 2`.

### Step 9: Live integration test — parallel quota with timing assertion

File: `/workspaces/codex-session/tests/account_quota_parallel.rs` (new file)

```rust
#[tokio::test]
async fn quota_multi_account_parallel_faster_than_sequential() {
    let env = TestEnv::new();
    env.seed_account("acct1", oauth_auth());
    env.seed_account("acct2", oauth_auth());
    env.seed_account("acct3", oauth_auth());

    let server = MockServer::start().await;

    // Each response delayed 1 second
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(payload(), "application/json")
                .set_delay(Duration::from_secs(1)),
        )
        .mount(&server)
        .await;

    let start = Instant::now();
    env.cmd()
        .env("CODEX_SESSION_WHAM_USAGE_URL", wham_url(&server))
        .args(["account", "quota", "--format", "json"])
        .timeout(Duration::from_secs(10))
        .assert()
        .success();
    let elapsed = start.elapsed();

    // Sequential would take >= 3s (3 accounts * 1s each)
    // Parallel should take ~1s (+ overhead)
    assert!(elapsed < Duration::from_secs(3),
        "expected parallel execution under 3s, took {:?}", elapsed);
}
```

> **MAJOR 5 — make this robust, not flaky.** A tight `elapsed < 3s` bound on shared CI runners flakes
> on cold-start, scheduler jitter, and `assert_cmd` subprocess spawn cost. Prefer asserting
> parallelism via **request concurrency** rather than pure wall-clock:
>
> - Bump the per-response delay (e.g. 2s) and the account count (e.g. 4), so sequential would be
>   ≥ 8s and assert a **generous** bound (`elapsed < 5s`). The gap between parallel and sequential
>   should be large enough to survive jitter.
> - Or (stronger) record received-request timestamps via a custom wiremock responder / a shared
>   counter and assert that all N requests arrived within a short window of each other — this proves
>   concurrency directly and is timing-jitter-resistant.
>   Keep at most one wall-clock test as a smoke signal; don't gate CI on a tight bound.

### Step 10: Live integration test — spinner suppression in JSON mode

File: `/workspaces/codex-session/tests/account_quota_parallel.rs` (same file)

```rust
#[tokio::test]
async fn quota_json_output_has_no_spinner_artifacts() {
    let env = TestEnv::new();
    env.seed_account("work", oauth_auth());

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(payload(), "application/json"))
        .mount(&server)
        .await;

    let output = env.cmd()
        .env("CODEX_SESSION_WHAM_USAGE_URL", wham_url(&server))
        .args(["account", "quota", "--format", "json"])
        .assert()
        .success()
        .get_output();

    // stdout must be valid JSON (no spinner escape sequences)
    let _: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    // stderr must not contain spinner characters
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains('\x1b'), "stderr contains ANSI escapes from spinners");
}
```

### Step 11: Live integration test — spinner suppression in piped (non-TTY) mode

The `assert_cmd` tests already run in a non-TTY context (subprocess with piped stdout/stderr). So
all existing tests implicitly verify that spinners don't corrupt output. Add an explicit assertion:

File: `/workspaces/codex-session/tests/account_health_parallel.rs`

```rust
#[tokio::test]
async fn health_piped_output_has_no_spinner_artifacts() {
    let env = TestEnv::new_empty();
    env.seed_account("work", TEST_AUTH);

    let output = env.cmd()
        .args(["account", "health", "--fast", "--format", "text"])
        .assert()
        .success()
        .get_output();

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Text output should contain the health table header but no ANSI spinner sequences
    assert!(stdout.contains("RANK") || stdout.contains("account"));
    assert!(!stdout.contains("⠋"), "stdout contains spinner frames");
}
```

### Step 12: Run full test suite and lints

```bash
just test
just lint
```

Verify all existing tests still pass and new tests pass.

### Step 13: Update plan index

Mark this round as `done` with today's date in the README.md execution order table:

File: `/workspaces/codex-session/.plan/01-todo/02-spinner-parallel-async-ux/README.md`

Update the Round 02 row's Status column from `todo` to `done` and fill in the Completed column.

## Acceptance Criteria

1. `docs/design/cli-style-guide.md` has a §9b spinner/progress spec (channel, suppression matrix,
   frame/color, finish markers, tracing coexistence, message conventions) and the §9 "only for quota"
   line no longer contradicts it (Step 0, BLOCKER 2)
2. `src/ui/spinner.rs` exists with `SpinnerGroup`, `SpinnerHandle`, and visibility helpers
3. `account health` (non-fast, multi-account) runs all accounts concurrently via `JoinSet`
4. Within each account's health check, quota fetch and heartbeat probe run concurrently via
   `tokio::join!`
5. `account quota` (multi-account) runs all accounts concurrently via `JoinSet`
6. Spinners are visible when: text format + stderr is TTY + not --fast
7. Spinners are hidden when: JSON format, or stderr is not TTY, or --fast
8. JSON output (`--format json`) is valid JSON with no spinner artifacts
9. Piped (non-TTY) text output has no ANSI escape sequences from spinners
10. `tracing` events emitted during async fetches do not corrupt spinner frames (MAJOR 3 — bridged or
    gated)
11. Multi-account output is deterministically ordered after the parallel collect (re-sorted by
    account id) — existing order/snapshot tests still pass (MINOR 8)
12. The spinner module reuses `ui::color` and implements the §9b style-guide spec from Step 0 (symbols,
    `[ok]`/`[err]` fallback, suppression matrix)
13. New integration tests pass and use real/hoisted helpers (no `cmd_raw`/`new_with_ping_profile`
    unless actually added); parallelism asserted via concurrency or a generous bound, not a tight
    wall-clock (MAJOR 4/5, MINOR 9)
14. All existing tests still pass (`just test`)
15. `just lint` passes

## Next Round

Round 03 adds spinners to doctor (step-by-step progress), account refresh/add (login flow
feedback), and handles edge cases (CTRL-C cleanup, error path spinners).
