# Round 01 — Async Runtime Migration

> Round 1 of 3 | Topic: Tokio runtime + async reqwest migration | Status: todo

## Context

codex-session is a Rust CLI wrapper around the `codex` binary that manages config-recipe-layered
sessions with multi-account credential pooling. Its network-facing commands (`account health`,
`account quota`) currently use `reqwest::blocking` with no async runtime. The project needs tokio
for parallel I/O (spinner + concurrent account checks in later rounds).

This round migrates the codebase from blocking to async: adds tokio as a production dependency,
switches reqwest from blocking to async, and makes all command/service functions async where needed.
No UX changes — behavior is identical. All existing tests must pass.

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

1. Add `tokio` to `[dependencies]` with `macros` + `rt-multi-thread` features
2. Add `indicatif` to `[dependencies]` (needed by Round 02, added now to avoid a second Cargo.toml
   change)
3. Switch `reqwest` features from `blocking` to default async (keep `rustls-tls`, `json`)
4. Update `main.rs` to use `#[tokio::main]`
5. Make `dispatch::run()` and all command handler functions async
6. Migrate `services/account/quota.rs`: `reqwest::blocking::Client` → `reqwest::Client`, add
   `.await` to `.send()`, `.bytes()`, etc. Replace `std::thread::sleep(1s)` retry delay with
   `tokio::time::sleep(1s).await`
7. Migrate `services/account/token_refresh.rs`: same blocking → async conversion
8. Migrate `services/account/gate.rs` `heartbeat_probe()`: `std::process::Command` →
   `tokio::process::Command`, replace poll loop with `tokio::time::timeout()` +
   `child.wait_with_output().await`
9. Migrate `gate::run_login()`: similar process spawn migration (but this is interactive — stdin
   must remain connected)
10. Update all call chains that invoke these async functions (add `.await`)
11. All existing tests pass (`just test`)
12. Lint passes (`just lint`)

### Out of scope

- Spinner UI (Round 02)
- Parallel execution (Round 02)
- Doctor progress indication (Round 03)
- Any behavioral changes to command output

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
reqwest = { version = "0.12", default-features = false, features = [
    "rustls-tls",
    "json",
] }

[dev-dependencies]
# Remove tokio from dev-dependencies (now in dependencies)
```

The `process` feature is needed for `tokio::process::Command` (gate.rs probe). The `time` feature
is needed for `tokio::time::sleep` and `tokio::time::timeout`.

### Step 2: Update main.rs to async

File: `/workspaces/codex-session/src/main.rs`

Add `#[tokio::main]` attribute and make `main()` async. The existing `main()` calls
`dispatch::run()` — add `.await` to that call. Everything else in main (arg parsing, logging setup,
exit code handling) stays synchronous.

### Step 3: Make dispatch::run() async

File: `/workspaces/codex-session/src/commands/dispatch.rs`

Change `pub(crate) fn run(...)` to `pub(crate) async fn run(...)`. Add `.await` to every command
handler call in the match arms. Helper functions `run_config()` and `run_config_recipe()` route to
local-only commands (no network) — make them async too for consistency (they just add `.await` to
their inner calls).

### Step 4: Make account command dispatch async

File: `/workspaces/codex-session/src/commands/account/mod.rs`

The `dispatch()` function routes to individual account command handlers. Make it async and add
`.await` to each handler call.

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

Functions that are purely local (cache reads, parsing) stay synchronous.

### Step 6: Migrate token_refresh service to async

File: `/workspaces/codex-session/src/services/account/token_refresh.rs`

- `pub(crate) fn refresh_token(...)` → `pub(crate) async fn refresh_token(...)`
- `reqwest::blocking::Client` → `reqwest::Client`
- `.post(...).json(...).send()` → `.post(...).json(...).send().await`
- `.json::<T>()` → `.json::<T>().await`

### Step 7: Migrate gate.rs heartbeat probe to async

File: `/workspaces/codex-session/src/services/account/gate.rs`

The `heartbeat_probe()` function (line 439) spawns `codex --profile ping exec --json "say ok"` and
polls it with a 15s timeout.

Changes:

- `fn heartbeat_probe(...)` → `async fn heartbeat_probe(...)`
- `use std::process::{Command, Stdio}` → `use tokio::process::Command` + `use std::process::Stdio`
- Remove the manual poll loop (`loop { child.try_wait()... sleep(200ms)... }`)
- Replace with:
  ```rust
  let output = tokio::time::timeout(
      PROBE_TIMEOUT,
      child.wait_with_output(),
  ).await;
  ```
- Handle timeout: `Err(_)` from timeout means probe took too long → kill child, return timeout
  message
- Handle success: `Ok(Ok(output))` → check output.status, combine stdout+stderr
- `pub(crate) fn probe_token(...)` → `pub(crate) async fn probe_token(...)`

The `run_login()` function also spawns a child process but is interactive (user interacts with codex
login). For login, use `tokio::process::Command` but with `.status().await` (no output capture
needed since stdio is inherited). The `run_login` function and its callers (`add.rs`, `refresh.rs`)
also become async.

### Step 8: Make command handlers async

The following command handler files need `async fn run(...)`:

- `/workspaces/codex-session/src/commands/account/health.rs` — `run()` and `build_entry()` become
  async (they call `quota::refresh().await` and `gate::probe_token().await`)
- `/workspaces/codex-session/src/commands/account/quota.rs` — `run()` and `fetch_view()` become
  async
- `/workspaces/codex-session/src/commands/account/refresh.rs` — `run()` becomes async (calls
  `gate::run_login().await`)
- `/workspaces/codex-session/src/commands/account/add.rs` — `run()` becomes async (calls
  `gate::run_login().await`)

Command handlers that are local-only (no network calls) can also be made async for consistency in
the dispatch match arms, or dispatch can `.await` only the async ones. Prefer consistency: make
all command handler `run()` functions async.

Each remaining command handler file gets `async fn run(...)` with no other changes:

- `commands/version.rs`
- `commands/completion.rs`
- `commands/config_status.rs`
- `commands/config_recipe_list.rs`
- `commands/config_recipe_show.rs`
- `commands/config_recipe_compose.rs`
- `commands/doctor.rs`
- `commands/pass_through.rs`
- `commands/account/list.rs`
- `commands/account/current.rs`
- `commands/account/use_.rs`
- `commands/account/remove.rs`
- `commands/account/cooldown.rs`

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

Fix any new clippy warnings introduced by the async migration. Common ones:

- `clippy::unused_async` on functions that are async but don't actually await (the local-only
  command handlers). Suppress with `#[allow(clippy::unused_async)]` on those functions — they're
  async for dispatch consistency and will gain awaits in later rounds.

### Step 13: Update plan index

Mark this round as `done` with today's date in the README.md execution order table:

File: `/workspaces/codex-session/.plan/01-todo/spinner-parallel-async-ux/README.md`

Update the Round 01 row's Status column from `todo` to `done` and fill in the Completed column.

## Acceptance Criteria

1. `just build` succeeds — no compilation errors
2. `just test` passes — all existing unit and integration tests pass with identical behavior
3. `just lint` passes — no new warnings (suppressed `unused_async` where appropriate)
4. `reqwest::blocking` is no longer used anywhere in `src/` (grep confirms zero matches)
5. `main.rs` uses `#[tokio::main]` and `async fn main()`
6. All command handlers are `async fn`
7. `services/account/quota.rs` uses `reqwest::Client` (not `reqwest::blocking::Client`)
8. `services/account/token_refresh.rs` uses `reqwest::Client`
9. `services/account/gate.rs` uses `tokio::process::Command` for both heartbeat probe and login
10. No behavioral changes — command output is byte-for-byte identical to the pre-migration state

## Next Round

Round 02 creates `src/ui/spinner.rs` and adds parallel execution with multi-spinner feedback to
`account health` and `account quota`.
