# Round 02 — Spinner Module + Parallel Account Health & Quota

> Round 2 of 3 | Topic: indicatif spinner helpers + parallel account operations | Status: todo

## Context

codex-session is a Rust CLI wrapper that manages multi-account credential pooling for the `codex`
binary. Commands like `account health` and `account quota` fetch data from remote APIs sequentially
per account with no progress feedback. After Round 01, the codebase uses tokio + async reqwest,
making parallel I/O and spinner integration straightforward.

This round creates a reusable spinner module built on `indicatif` and redesigns `account health` and
`account quota` to run all account operations in parallel with per-account spinner feedback.

## Current State

After Round 01, the following is true:

- Tokio runtime is active (`#[tokio::main]` in main.rs)
- All command handlers and services are `async fn`
- `reqwest::Client` (async) replaces `reqwest::blocking::Client`
- `tokio::process::Command` replaces `std::process::Command` in gate.rs
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

**Round 01** migrated the codebase from blocking to async:

- Added tokio + indicatif to Cargo.toml
- Switched reqwest from blocking to async
- Made all command handlers and services async
- All tests pass, behavior unchanged

## Scope of This Round

### In scope

1. Create `src/ui/spinner.rs` module with reusable spinner helpers
2. Redesign `account health` for parallel execution with multi-spinner
3. Redesign `account quota` for parallel execution with multi-spinner
4. Suppress spinners when `--format json` or non-TTY (piped output)
5. Live integration tests for parallelism correctness and output integrity

### Out of scope

- Doctor progress indication (Round 03)
- Account refresh/add spinners (Round 03)
- Design system styling changes (separate plan)
- Error path spinners (Round 03)

## Implementation Steps

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
        entries.push(result?);
    }

    // ... existing sort, rank, render logic ...
}
```

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
    }

    // ... existing sort, rank, render logic ...
}
```

### Step 6: Add spinner suppression logic

The spinner visibility decision must account for:

1. **Output format**: When `--format json`, spinners are hidden (JSON output must be clean)
2. **TTY detection**: When stderr is not a terminal (piped), spinners are hidden
3. **`--fast` flag** (health only): When fast mode, no network calls → no spinners

Create a helper function in `spinner.rs`:

```rust
pub(crate) fn should_show_spinner(format: crate::cli::OutputFormat) -> bool {
    matches!(format, crate::cli::OutputFormat::Text)
        && std::io::stderr().is_terminal()
}
```

Command handlers call this to decide the `visible` parameter for `SpinnerGroup::new()`.

### Step 7: Handle indicatif + stderr interleaving

If the command needs to write warnings to stderr (via `ctx.ui.write_warning()`) while spinners are
active, the output will interleave. Use `MultiProgress::suspend()` to temporarily pause spinner
rendering:

```rust
impl SpinnerGroup {
    /// Suspend spinner rendering, execute a closure, then resume.
    /// Use this when writing to stderr outside of the spinner system.
    pub(crate) fn suspend<F, R>(&self, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        self.multi.suspend(f)
    }
}
```

### Step 8: Live integration test — parallel health with wiremock

File: `/workspaces/codex-session/tests/account_health_parallel.rs` (new file)

Test that multi-account health runs all accounts concurrently by using wiremock with deliberate
delays:

```rust
#[tokio::test]
async fn health_multi_account_runs_in_parallel() {
    let env = TestEnv::new_with_ping_profile();
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

Note: `--fast` skips network calls and reads from cache, so the parallel timing test needs to use
non-fast mode. However, non-fast health also runs a heartbeat probe which spawns `codex`. For a
clean test, either:

- Mock the codex binary with a stub that sleeps (using the existing fixture pattern)
- Test only account quota parallelism (which doesn't need a codex binary)

Prefer testing with `account quota` for the parallelism timing assertion, and test `account health`
for correctness (output format, all accounts present).

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

File: `/workspaces/codex-session/.plan/01-todo/spinner-parallel-async-ux/README.md`

Update the Round 02 row's Status column from `todo` to `done` and fill in the Completed column.

## Acceptance Criteria

1. `src/ui/spinner.rs` exists with `SpinnerGroup`, `SpinnerHandle`, and visibility helpers
2. `account health` (non-fast, multi-account) runs all accounts concurrently via `JoinSet`
3. Within each account's health check, quota fetch and heartbeat probe run concurrently via
   `tokio::join!`
4. `account quota` (multi-account) runs all accounts concurrently via `JoinSet`
5. Spinners are visible when: text format + stderr is TTY + not --fast
6. Spinners are hidden when: JSON format, or stderr is not TTY, or --fast
7. JSON output (`--format json`) is valid JSON with no spinner artifacts
8. Piped (non-TTY) text output has no ANSI escape sequences from spinners
9. New integration tests pass: parallel timing assertion, spinner suppression
10. All existing tests still pass (`just test`)
11. `just lint` passes

## Next Round

Round 03 adds spinners to doctor (step-by-step progress), account refresh/add (login flow
feedback), and handles edge cases (CTRL-C cleanup, error path spinners).
