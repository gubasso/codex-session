# Round 01 — Async Runtime Migration

> Round 1 of 3 | Topic: Tokio runtime + async reqwest migration | Status: todo

## Context

codex-session is a Rust CLI wrapper around the `codex` binary that manages config-recipe-layered
sessions with multi-account credential pooling. Its network-facing commands (`account health`,
`account quota`) currently use `reqwest::blocking` with no async runtime. The project needs tokio
for parallel I/O (spinner + concurrent account checks in later rounds).

This round migrates the **network surface** from blocking to async: adds tokio as a production
dependency, switches the quota reqwest client from blocking to async, and makes the `account health`
/ `account quota` handlers and the quota service async. No UX changes — behavior is identical. All
existing tests must pass.

> **Plan-review scope correction (BLOCKER 1).** The original draft made `token_refresh` async and
> claimed every handler — including `pass_through` — becomes async "with no other changes." That is
> wrong: `token_refresh::refresh_token` is called by `src/services/account/retry.rs:223`
> (`try_refresh`), and `retry::run_auto` / `single_attempt` are the **main `codex` exec hot path**
> (`pass_through.rs:82,85,614,616`), which installs `SignalSession` and runs a synchronous,
> stdio-inherited blocking child wait. Making `token_refresh` async would cascade through all of it.
>
> **Decision:** keep `token_refresh.rs`, `retry.rs`, `pass_through.rs`, `run_child`, and
> `adapters/spawner.rs` **synchronous**. `token_refresh` keeps `reqwest::blocking` and is invoked
> from the async quota service via `tokio::task::spawn_blocking`. `dispatch::run` becomes async but
> routes the External/None (pass_through) arm to the existing sync path (sync is callable from
> async). Only the two network _query_ commands and the quota service become truly async.

## Current State

### Cargo.toml dependencies (blocking reqwest, tokio only in dev-deps)

File: `/workspaces/codex-session/Cargo.toml`

```toml
[dependencies]
# ... other deps ...
reqwest = { version = "0.12", default-features = false, features = [
    "blocking",
    "rustls-tls",
    "json",
] }

[dev-dependencies]
# tokio is already here for wiremock tests
wiremock = "0.6"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

### main.rs entry point (synchronous)

File: `/workspaces/codex-session/src/main.rs` — currently a synchronous `fn main()` that calls
`dispatch::run()`.

### dispatch.rs (synchronous routing)

File: `/workspaces/codex-session/src/commands/dispatch.rs`

```rust
pub(crate) fn run(ctx: &context::AppContext, cli: cli::Cli) -> Result<u8, error::AppError> {
    match cli.command {
        Some(cli::Commands::Doctor(args)) => commands::doctor::run(ctx, args),
        Some(cli::Commands::Account(args)) => commands::account::dispatch(ctx, args).map(|()| 0),
        // ... other arms ...
    }
}
```

### HTTP-calling services (all blocking)

File: `/workspaces/codex-session/src/services/account/quota.rs`

```rust
fn fetch_inner(ctx: &crate::context::AppContext, account: &AccountId) -> Result<QuotaResult, QuotaError> {
    // ...
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;

    for attempt in 0..=1 {
        let response = client.get(url)
            .header(reqwest::header::AUTHORIZATION, format!("Bearer {access_token}"))
            // ... headers ...
            .send()?;
        // retry logic for 5xx with std::thread::sleep(1s)
    }
}
```

File: `/workspaces/codex-session/src/services/account/token_refresh.rs` — uses
`reqwest::blocking::Client` for OAuth token refresh POST.

### gate.rs heartbeat probe (blocking process spawn)

File: `/workspaces/codex-session/src/services/account/gate.rs` (lines 439-533)

```rust
fn heartbeat_probe(ctx: &AppContext, account: &AccountId) -> Result<(Option<bool>, String), AppError> {
    // Uses std::process::Command to spawn `codex --profile ping exec --json "say ok"`
    // Polls child.try_wait() in a loop with std::thread::sleep(200ms)
    // 15-second timeout via Instant::now() + elapsed check
    let mut child = Command::new(binary.as_std_path())
        .args(["--profile", PING_PROFILE, "exec", "--json", "say ok"])
        // ... env setup ...
        .spawn()?;

    let start = Instant::now();
    let status = loop {
        match child.try_wait()? {
            Some(status) => break status,
            None if start.elapsed() >= PROBE_TIMEOUT => {
                let _ = child.kill();
                // ...
            }
            None => std::thread::sleep(Duration::from_millis(200)),
        }
    };
}
```

### Command handlers calling these services (all synchronous)

- `/workspaces/codex-session/src/commands/account/health.rs` — calls `quota::refresh()` and
  `gate::probe_token()` synchronously per account in a for loop (line 53)
- `/workspaces/codex-session/src/commands/account/quota.rs` — calls `quota::refresh()` per account
  in a for loop (line 52)
- `/workspaces/codex-session/src/commands/account/refresh.rs` — calls `gate::run_login()`
- `/workspaces/codex-session/src/commands/account/add.rs` — calls `gate::run_login()`
- `/workspaces/codex-session/src/commands/doctor.rs` — all local (no network), no async needed yet

### Existing test infrastructure

Integration tests use `assert_cmd` (subprocess-based) and `wiremock` (async mock server). Tests
that use wiremock already have `#[tokio::test]`:

File: `/workspaces/codex-session/tests/account_quota_http.rs`

```rust
#[tokio::test]
async fn retries_once_on_5xx_then_success() {
    let env = TestEnv::new();
    add_account(&env, "work");
    let server = MockServer::start().await;
    // ... assert_cmd runs the binary as subprocess ...
}
```

## Previous Rounds

This is the first round. No prior rounds.

## Scope of This Round

### In scope

1. Add `tokio` to `[dependencies]` with `macros`, `rt-multi-thread`, `process`, `time` features
2. Add `indicatif` to `[dependencies]` (needed by Round 02, added now to avoid a second Cargo.toml
   change)
3. **Keep `reqwest`'s `blocking` feature** (still used by `token_refresh.rs`) **and also enable async
   default features** (keep `rustls-tls`, `json`). The quota client switches to async; the
   token-refresh client stays blocking.
4. Update `main.rs` to use `#[tokio::main]`
5. Make `dispatch::run()` async. Make **only the network query handlers** async: `account health`,
   `account quota`. Other handlers stay sync; dispatch calls sync handlers without `.await`. The
   `pass_through` (External/None) arm stays sync.
6. Migrate `services/account/quota.rs`: `reqwest::blocking::Client` → `reqwest::Client`, add
   `.await` to `.send()`, `.bytes()`, etc. Replace `std::thread::sleep(1s)` retry delay with
   `tokio::time::sleep(1s).await`. The in-fetch token refresh (`quota.rs:120`) calls
   `token_refresh::refresh_token` via `tokio::task::spawn_blocking`.
7. **Do NOT migrate `services/account/token_refresh.rs`.** It keeps `reqwest::blocking`; only its
   call site in the async quota service wraps it in `spawn_blocking`. `retry.rs:223` keeps calling it
   directly and synchronously (retry stays sync).
8. Migrate `services/account/gate.rs` `heartbeat_probe()`: `std::process::Command` →
   `tokio::process::Command` with **`.kill_on_drop(true)`**, replacing the poll loop with
   `tokio::time::timeout()` (see Step 7 below for the correct timeout-kill-drain pattern).
9. **Leave `gate::run_login()` synchronous** (it is interactive, stdio-inherited, and only reached
   from the sync refresh/add handlers; Round 03 wraps spinners _around_ it, not inside async). If a
   later need arises it can move to `spawn_blocking`, but no async conversion this round.
10. Update the call chains for the now-async functions (`quota::refresh`, `gate::probe_token`,
    health/quota handlers) — add `.await`.
11. All existing tests pass (`just test`)
12. Lint passes (`just lint`)

### Out of scope

- Spinner UI (Round 02)
- Parallel execution (Round 02)
- Doctor progress indication (Round 03)
- Any behavioral changes to command output
- **`retry.rs`, `pass_through.rs`, `run_child`, `adapters/spawner.rs`, `token_refresh.rs`,
  `gate::run_login` — all stay synchronous** (BLOCKER 1 decision)

## Implementation Steps

### Step 1: Update Cargo.toml dependencies

File: `/workspaces/codex-session/Cargo.toml`

Move `tokio` from `[dev-dependencies]` to `[dependencies]`. Add `indicatif`. Change reqwest
features.

Before:

```toml
[dependencies]
reqwest = { version = "0.12", default-features = false, features = [
    "blocking",
    "rustls-tls",
    "json",
] }

[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

After:

```toml
[dependencies]
tokio = { version = "1", features = ["macros", "rt-multi-thread", "process", "time"] }
indicatif = "0.17"
# Keep `blocking` — token_refresh.rs still uses reqwest::blocking (called via spawn_blocking).
# Async default features are added for the quota client.
reqwest = { version = "0.12", default-features = false, features = [
    "blocking",
    "rustls-tls",
    "json",
] }

[dev-dependencies]
# Remove tokio from dev-dependencies (now in dependencies)
```

The `process` feature is needed for `tokio::process::Command` (gate.rs probe). The `time` feature
is needed for `tokio::time::sleep` and `tokio::time::timeout`. `blocking` stays because
`token_refresh.rs` is intentionally not migrated (BLOCKER 1 decision).

> Verification hook: after this round, `grep -rn "reqwest::blocking" src/` should match **only**
> `token_refresh.rs`, and that file's caller in the quota service must be inside `spawn_blocking`.

### Step 2: Update main.rs to async

File: `/workspaces/codex-session/src/main.rs`

Add `#[tokio::main]` attribute and make `main()` async. The existing `main()` calls
`dispatch::run()` — add `.await` to that call. Everything else in main (arg parsing, logging setup,
exit code handling) stays synchronous.

### Step 3: Make dispatch::run() async

File: `/workspaces/codex-session/src/commands/dispatch.rs`

Change `pub(crate) fn run(...)` to `pub(crate) async fn run(...)`. Add `.await` only to the arms that
became async (the `Account` arm, via `account::dispatch`). The `External` / `None` (pass_through)
arms stay **synchronous** — call them without `.await`. Local-only helpers `run_config()` /
`run_config_recipe()` stay sync.

### Step 4: Make account command dispatch async (selectively)

File: `/workspaces/codex-session/src/commands/account/mod.rs`

Make `dispatch()` async. Add `.await` only for the handlers that became async (`health::run`,
`quota::run`). The other arms (`add`, `list`, `current`, `remove`, `refresh`, `cooldown`) stay sync
and are called without `.await`. (An async dispatch may call sync handlers directly — no need to make
every handler async just for uniformity.)

### Step 5: Migrate quota service to async

File: `/workspaces/codex-session/src/services/account/quota.rs`

Key changes:

- `pub(crate) fn refresh(...)` → `pub(crate) async fn refresh(...)`
- `fn fetch(...)` → `async fn fetch(...)`
- `fn fetch_inner(...)` → `async fn fetch_inner(...)`
- `reqwest::blocking::Client::builder()` → `reqwest::Client::builder()`
- `.send()` → `.send().await`
- `.bytes()` → `.bytes().await`
- `std::thread::sleep(Duration::from_secs(1))` → `tokio::time::sleep(Duration::from_secs(1)).await`
- `pub(crate) fn get(...)` → `pub(crate) async fn get(...)` (calls refresh)
- The in-fetch token refresh at `quota.rs:120` (`super::token_refresh::refresh_token(&auth_path)`)
  stays a blocking call — wrap it in `tokio::task::spawn_blocking`:
  ```rust
  let auth_path = auth_path.clone();
  let refreshed = tokio::task::spawn_blocking(move || {
      super::token_refresh::refresh_token(&auth_path)
  }).await?; // JoinError -> map to QuotaError as appropriate
  ```

Functions that are purely local (cache reads, parsing) stay synchronous.

### Step 6: Do NOT migrate token_refresh (keep blocking)

File: `/workspaces/codex-session/src/services/account/token_refresh.rs` — **unchanged.**

`refresh_token` keeps `reqwest::blocking::Client` (Cargo `blocking` feature retained). It has two
callers:

- `src/services/account/quota.rs:120` (async context) → call via `spawn_blocking` (see Step 5).
- `src/services/account/retry.rs:223` (`try_refresh`, sync context) → unchanged, direct call.

This is the core of the BLOCKER 1 decision: it keeps `retry.rs` and the entire `pass_through` exec /
signal-forwarding path synchronous and untouched.

### Step 7: Migrate gate.rs heartbeat probe to async

File: `/workspaces/codex-session/src/services/account/gate.rs`

The `heartbeat_probe()` function (line 439) spawns `codex --profile ping exec --json "say ok"` and
polls it with a 15s timeout.

Changes:

- `fn heartbeat_probe(...)` → `async fn heartbeat_probe(...)`
- `use std::process::{Command, Stdio}` → `use tokio::process::Command` + `use std::process::Stdio`
- Add `.kill_on_drop(true)` to the `Command` builder.
- Remove the manual poll loop (`loop { child.try_wait()... sleep(200ms)... }`).

> **MINOR 6 — correct timeout-kill pattern.** `child.wait_with_output()` **moves** the `Child`, so on
> the timeout branch you no longer hold a handle to `.kill()`. Two safe options:
>
> **Option A (preferred) — keep the child, kill explicitly, then drain:**
>
> ```rust
> match tokio::time::timeout(PROBE_TIMEOUT, child.wait()).await {
>     Ok(Ok(status)) => {
>         // read piped stdout/stderr after wait
>     }
>     Ok(Err(e)) => { /* spawn/io error */ }
>     Err(_elapsed) => {
>         let _ = child.start_kill();
>         let _ = child.wait().await;
>         // return the timeout message, draining whatever output exists
>     }
> }
> ```
>
> **Option B — `kill_on_drop(true)` + `wait_with_output()`:** on timeout the dropped future reaps the
> child via kill-on-drop, but you lose captured output for the timeout message. Prefer Option A to
> preserve the existing drain-on-timeout behavior (`drain_child_output`).

- `pub(crate) fn probe_token(...)` → `pub(crate) async fn probe_token(...)`

**`run_login()` stays synchronous.** It is interactive (stdio inherited), only reached from the sync
`add.rs` / `refresh.rs` handlers, and Round 03 wraps spinners _around_ it (clearing before the
interactive child runs), not inside an async flow. Do not convert it this round.

### Step 8: Make only the network handlers async

Exactly two command handlers become async this round:

- `/workspaces/codex-session/src/commands/account/health.rs` — `run()` and `build_entry()` become
  async (they call `quota::refresh().await` and `gate::probe_token().await`)
- `/workspaces/codex-session/src/commands/account/quota.rs` — `run()` and `fetch_view()` become
  async

**All other handlers stay synchronous** — do NOT add `async`/`.await` to them. An async dispatch can
call sync handlers directly. This avoids `clippy::unused_async` churn and, critically, keeps
`pass_through.rs` (and the `retry.rs` exec/signal path it drives) synchronous:

- stays sync: `version`, `completion`, `config_status`, `config_recipe_*`, `doctor`,
  **`pass_through`**, `account/{list,current,use_,remove,cooldown,refresh,add}`

> The original draft listed `pass_through.rs` here as "async with no other changes" — removed. It
> drives `run_child` (a blocking child wait) and `retry::run_auto`/`single_attempt`; it must remain
> synchronous (BLOCKER 1).

### Step 9: Update helper functions in health.rs and quota.rs

In `health.rs`:

- `fn fetch_quota(...)` → `async fn fetch_quota(...)` (calls `quota::refresh().await`)
- `fn fetch_probe(...)` → `async fn fetch_probe(...)` (calls `gate::probe_token().await`)
- `fn build_entry(...)` → `async fn build_entry(...)` (calls the above)

In `quota.rs` (command):

- `fn fetch_view(...)` → `async fn fetch_view(...)` (calls `quota::refresh().await`)

### Step 10: Verify and fix compilation

Run `just build` to catch any remaining synchronous calls to now-async functions. The compiler will
flag every missing `.await` as an error. Fix all compilation errors.

Pay special attention to:

- Closures that call async functions (may need to become async closures or be refactored)
- `map_or_else` / `map` chains that call async functions (need to be converted to match/if-let)
- Any trait implementations that call async functions

### Step 11: Run all tests

```bash
just test
```

All existing tests must pass. The integration tests use `assert_cmd` which runs the binary as a
subprocess — they don't care whether the binary is internally async or not. The wiremock-based tests
already use `#[tokio::test]`.

### Step 12: Run lints

```bash
just lint
```

Fix any new clippy warnings introduced by the async migration. Because only the two network handlers
became async (and both genuinely `.await`), `clippy::unused_async` should not appear — if it does,
the wrong function was made async. Do not blanket-`#[allow]` it.

### Step 13: Update plan index

Mark this round as `done` with today's date in the README.md execution order table:

File: `/workspaces/codex-session/.plan/01-todo/02-spinner-parallel-async-ux/README.md`

Update the Round 01 row's Status column from `todo` to `done` and fill in the Completed column.

## Acceptance Criteria

1. `just build` succeeds — no compilation errors
2. `just test` passes — all existing unit and integration tests pass with identical behavior
3. `just lint` passes — no new warnings (no blanket `unused_async` suppressions needed)
4. `reqwest::blocking` is used **only** in `token_refresh.rs`; its async-context caller
   (`quota.rs:120`) goes through `spawn_blocking` (grep confirms the single match)
5. `main.rs` uses `#[tokio::main]` and `async fn main()`
6. Only `account health` and `account quota` handlers are `async fn`; all others remain sync
7. `services/account/quota.rs` uses `reqwest::Client` (not `reqwest::blocking::Client`)
8. `services/account/token_refresh.rs` is **unchanged** (still `reqwest::blocking`)
9. `services/account/gate.rs` uses `tokio::process::Command` for the heartbeat probe with the
   Option-A timeout-kill-drain pattern; `run_login` stays `std::process::Command` (synchronous)
10. `retry.rs`, `pass_through.rs`, `run_child`, `adapters/spawner.rs` are unchanged (synchronous)
11. No behavioral changes — command output is byte-for-byte identical to the pre-migration state;
    `codex` exec passthrough and Ctrl-C → child forwarding still work (regression guard)

## Next Round

Round 02 creates `src/ui/spinner.rs` and adds parallel execution with multi-spinner feedback to
`account health` and `account quota`.
