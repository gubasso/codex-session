# Round 03 — Doctor, Refresh, Add Spinners + Polish

> Round 3 of 3 | Topic: Extended spinner coverage + edge cases + live tests | Status: todo

## Context

codex-session is a Rust CLI wrapper that manages multi-account credential pooling for the `codex`
binary. After Rounds 01 and 02, the codebase uses tokio async, has a reusable spinner module
(`src/ui/spinner.rs`), and runs `account health` and `account quota` in parallel with multi-spinner
feedback. This final round extends spinner coverage to `doctor`, `account refresh`, and `account
add`, then handles edge cases and adds comprehensive live tests.

## Current State

After Rounds 01 and 02:

- Tokio runtime active, all services and commands are async
- `src/ui/spinner.rs` provides `SpinnerGroup` and `SpinnerHandle` with TTY/format-aware visibility
- `account health` runs accounts in parallel with per-account spinners, quota+probe concurrent
- `account quota` runs accounts in parallel with spinners
- Spinners suppressed in JSON mode and non-TTY contexts

### doctor command (local checks, no progress indication)

File: `/workspaces/codex-session/src/commands/doctor.rs`

The doctor command runs sequential validation checks (codex binary resolution, config-recipe
validation, account auth verification, layer checks, etc.). Each check is local (filesystem reads)
and fast individually, but there are many checks. The command currently produces output only after
all checks complete — no intermediate feedback.

The doctor report structure:

```rust
pub(crate) struct DoctorReport {
    pub(crate) checks: Vec<CheckResult>,
    pub(crate) summary: CheckSummary,
    pub(crate) next_steps: Vec<String>,
    // ... other fields ...
}
```

Each `CheckResult` has `name`, `status` (Ok/Warn/Fail), and `detail`.

### account refresh command (interactive login flow)

File: `/workspaces/codex-session/src/commands/account/refresh.rs`

Calls `gate::run_login()` which spawns an interactive `codex login` process. The user interacts
with the child process directly (browser-based OAuth flow). After login completes, the command
copies the new auth.json to the account directory.

### account add command (interactive login flow)

File: `/workspaces/codex-session/src/commands/account/add.rs`

Similar to refresh — creates account directory, then calls `gate::run_login()` for the initial
login.

### signal handling (existing)

The project uses `signal-hook` for signal handling (`signal-hook = "0.3"` in Cargo.toml).

## Previous Rounds

**Round 01** migrated from blocking to async: tokio runtime, async reqwest, async command handlers.

**Round 02** created `src/ui/spinner.rs` and added parallel execution with multi-spinner to
`account health` and `account quota`. Spinners auto-suppress in JSON/non-TTY.

## Scope of This Round

### In scope

1. Doctor command step-by-step spinner progress
2. Account refresh spinner during login wait
3. Account add spinner during login wait
4. Signal handling integration (clean spinner teardown on CTRL-C)
5. Error path spinner cleanup (ensure spinners don't linger on errors)
6. Live integration tests for doctor progress, refresh/add flows, and edge cases
7. Final plan completion (move plan directory to `.plan/02-done/`)

### Out of scope

- Changing doctor check logic or adding new checks
- Changing login flow logic
- Design system styling (separate plan)
- Any changes to `account health` or `account quota` (completed in Round 02)

## Implementation Steps

### Step 1: Add spinner to doctor command

File: `/workspaces/codex-session/src/commands/doctor.rs`

The doctor command runs checks sequentially and builds a `DoctorReport`. Add a spinner that shows
which check is currently running:

```rust
pub(crate) async fn run(ctx: &AppContext, args: DoctorArgs) -> Result<u8, AppError> {
    let show_spinner = args.format == OutputFormat::Text && std::io::stderr().is_terminal();
    let spinners = SpinnerGroup::new(show_spinner);

    let spinner = spinners.add("Running doctor checks...");

    // Before each check, update the spinner message:
    spinner.set_message("Checking codex binary...");
    let codex_check = check_codex_binary(ctx);
    checks.push(codex_check);

    spinner.set_message("Checking config recipe...");
    let recipe_check = check_config_recipe(ctx);
    checks.push(recipe_check);

    spinner.set_message("Checking account auth...");
    // ... more checks ...

    // After all checks, finish the spinner with summary
    let summary = compute_summary(&checks);
    if summary.fail > 0 {
        spinner.finish_err(&format!("{} checks failed", summary.fail));
    } else if summary.warn > 0 {
        spinner.finish_ok(&format!("All checks passed ({} warnings)", summary.warn));
    } else {
        spinner.finish_ok("All checks passed");
    }

    // Render the full report (after spinner is finished)
    ctx.ui.write_doctor(&report, args.format)?;
    // ...
}
```

The spinner shows a single rolling message that updates as each check runs. This is simpler than
multi-spinner (doctor checks are sequential and local, not parallel network calls).

### Step 2: Add spinner to account refresh command

File: `/workspaces/codex-session/src/commands/account/refresh.rs`

The refresh command calls `gate::run_login()` which spawns an interactive child process. The spinner
should show _before_ the login prompt (while setting up the environment) and _after_ the login
completes (while copying auth files), but NOT during the interactive login itself.

```rust
pub(crate) async fn run(ctx: &AppContext, args: RefreshArgs) -> Result<(), AppError> {
    let show_spinner = std::io::stderr().is_terminal();
    let spinners = SpinnerGroup::new(show_spinner);

    let spinner = spinners.add(&format!("Preparing login for \"{}\"...", account));

    // ... setup code (resolving paths, validating state) ...

    // Clear spinner before interactive login (don't interfere with child's terminal)
    spinner.finish_and_clear();

    // Interactive login (user sees codex login's own output)
    let exit_code = gate::run_login(ctx, &opts).await?;

    if exit_code == 0 {
        let spinner = spinners.add("Saving credentials...");
        // ... copy auth.json, verify ...
        spinner.finish_ok("Credentials refreshed");
    } else {
        // Login failed — no spinner needed, just report
    }
}
```

Key: The spinner must be cleared before spawning the interactive child process. Otherwise spinner
escape codes will corrupt the child's terminal output.

### Step 3: Add spinner to account add command

File: `/workspaces/codex-session/src/commands/account/add.rs`

Same pattern as refresh:

```rust
pub(crate) async fn run(ctx: &AppContext, args: AddArgs) -> Result<(), AppError> {
    let show_spinner = std::io::stderr().is_terminal();
    let spinners = SpinnerGroup::new(show_spinner);

    let spinner = spinners.add(&format!("Setting up account \"{}\"...", name));

    // ... create account directory, validate name ...

    spinner.finish_and_clear();

    // Interactive login
    let exit_code = gate::run_login(ctx, &opts).await?;

    if exit_code == 0 {
        let spinner = spinners.add("Saving account...");
        // ... finalize account setup ...
        spinner.finish_ok(&format!("Account \"{}\" added", name));
    }
}
```

### Step 4: Ensure spinner cleanup on error paths

Review all command handlers that use spinners and ensure that spinners are properly finished or
cleared when an error occurs. The `SpinnerHandle`'s `Drop` implementation should call
`finish_and_clear()` automatically if the spinner was not explicitly finished.

File: `/workspaces/codex-session/src/ui/spinner.rs`

Add a `Drop` impl for `SpinnerHandle`:

```rust
impl Drop for SpinnerHandle {
    fn drop(&mut self) {
        if !self.finished {
            self.bar.finish_and_clear();
        }
    }
}
```

This ensures that if a function returns early (via `?` or panic), the spinner is cleaned up and the
terminal is restored to a normal state.

### Step 5: Signal handling for spinner cleanup

The project uses `signal-hook` for signal handling. When the user presses CTRL-C during a spinner
operation, the spinner should be cleaned up before the process exits.

Check the existing signal handling in the project. If signals are already handled gracefully (the
process exits cleanly on SIGINT), then the `Drop` implementation from Step 4 is sufficient — Rust
drops all stack-allocated values during unwinding.

If the process uses `std::process::exit()` on signals (which skips Drop), the spinner may leave
the terminal in a bad state. In that case, ensure the signal handler calls
`MultiProgress::clear()` before exiting.

File: `/workspaces/codex-session/src/ui/spinner.rs`

If needed, add a global cleanup function:

```rust
/// Call from signal handlers to clean up any active spinners.
/// Safe to call multiple times.
pub(crate) fn cleanup_all_spinners() {
    // indicatif's ProgressBar::abandon() marks the bar as finished
    // without printing a final message, which is appropriate for signal cleanup.
}
```

However, `indicatif` handles terminal restoration internally when `ProgressBar` is dropped. Test
whether CTRL-C during spinner leaves the terminal clean. If it does, skip this step.

### Step 6: Live integration test — doctor with progress

File: `/workspaces/codex-session/tests/cmd_doctor_spinner.rs` (new file)

Test that doctor still produces correct output and that spinner artifacts don't appear in non-TTY
output:

```rust
#[test]
fn doctor_piped_output_is_clean() {
    let env = TestEnv::new();

    let output = env.cmd()
        .args(["doctor", "--format", "text"])
        .assert()
        .get_output();

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Doctor should contain check results
    assert!(stdout.contains("OK") || stdout.contains("WARN") || stdout.contains("FAIL"));
    // No spinner frame characters in stdout
    assert!(!stdout.contains("⠋"), "spinner frame leaked to stdout");
    assert!(!stdout.contains("⠙"), "spinner frame leaked to stdout");
}

#[test]
fn doctor_json_output_is_valid() {
    let env = TestEnv::new();

    let output = env.cmd()
        .args(["doctor", "--format", "json"])
        .assert()
        .get_output()
        .stdout
        .clone();

    // Must be valid JSON, no spinner artifacts
    let _: serde_json::Value = serde_json::from_slice(&output).unwrap();
}
```

### Step 7: Live integration test — refresh spinner suppression

File: `/workspaces/codex-session/tests/account_refresh_spinner.rs` (new file)

Testing the refresh flow requires a mock codex binary. Use the existing fixture pattern:

```rust
#[test]
fn refresh_piped_mode_no_spinner_artifacts() {
    let env = TestEnv::new();
    env.seed_account("work", TEST_AUTH);
    // Use a stub codex that simulates successful login
    with_stub_child(env.cmd(), "fake-codex-login-success.sh");

    let output = env.cmd()
        .args(["account", "refresh", "--account", "work"])
        .assert()
        .get_output();

    let stderr = String::from_utf8_lossy(&output.stderr);
    // In piped mode, no spinner escape sequences
    assert!(!stderr.contains('\x1b'), "stderr has ANSI escapes from spinners in piped mode");
}
```

Note: This test requires a fake-codex fixture that simulates a successful login. If one doesn't
already exist, create a minimal shell script fixture:

File: `/workspaces/codex-session/tests/fixtures/fake-codex-login-success.sh`

```bash
#!/usr/bin/env bash
# Stub: simulates successful codex login by writing auth.json to CODEX_HOME
if [[ "$1" == "login" ]] || [[ "$*" == *"login"* ]]; then
    cat > "$CODEX_HOME/auth.json" <<'JSON'
{"tokens":{"access_token":"new-token","account_id":"acct-123","plan":"pro"}}
JSON
    exit 0
fi
exit 1
```

Check if such a fixture already exists before creating a new one.

### Step 8: Live integration test — error path spinner cleanup

File: `/workspaces/codex-session/tests/account_health_parallel.rs` (add to existing from Round 02)

Test that when a quota fetch fails, the spinner is cleaned up and the error is reported:

```rust
#[tokio::test]
async fn health_with_failing_account_cleans_up_spinner() {
    let env = TestEnv::new_empty();
    env.seed_account("good", oauth_auth());
    env.seed_account("bad", r#"{"tokens":{}}"#); // Missing access_token

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(payload(), "application/json"))
        .mount(&server)
        .await;

    let output = env.cmd()
        .env("CODEX_SESSION_WHAM_USAGE_URL", wham_url(&server))
        .args(["account", "health", "--fast", "--format", "json"])
        .assert()
        .success()
        .get_output();

    // Both accounts should appear in output (one with error status)
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let arr = value.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    // No spinner artifacts in stderr
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains('\x1b'), "spinner artifacts in stderr");
}
```

### Step 9: Live integration test — CTRL-C signal handling

File: `/workspaces/codex-session/tests/account_health_parallel.rs` (add to existing)

Test that sending SIGINT during a health check doesn't leave terminal artifacts. This is harder to
test automatically but can be approximated:

```rust
#[tokio::test]
async fn health_killed_during_fetch_exits_cleanly() {
    let env = TestEnv::new_empty();
    env.seed_account("slow", oauth_auth());

    let server = MockServer::start().await;
    // Response that takes 30 seconds (longer than test timeout)
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/usage"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(payload(), "application/json")
                .set_delay(Duration::from_secs(30)),
        )
        .mount(&server)
        .await;

    // Start the command and kill it after 1 second
    let mut child = env.cmd_raw()
        .env("CODEX_SESSION_WHAM_USAGE_URL", wham_url(&server))
        .args(["account", "health", "--fast", "--format", "text"])
        .spawn()
        .unwrap();

    tokio::time::sleep(Duration::from_secs(1)).await;

    // Send SIGINT (Ctrl-C equivalent)
    unsafe { libc::kill(child.id() as i32, libc::SIGINT); }

    let status = child.wait().unwrap();
    // Process should exit (not hang)
    assert!(!status.success() || status.code() == Some(130)); // 130 = 128 + SIGINT
}
```

Note: This test needs `cmd_raw()` which returns `std::process::Command` instead of
`assert_cmd::Command`. Check if `TestEnv` has this method; if not, add a simple helper.

### Step 10: Review spinner messages for UX quality

Review all spinner messages across all commands for consistency, clarity, and tone:

| Command           | Phase              | Spinner Message                                        |
| ----------------- | ------------------ | ------------------------------------------------------ |
| `account health`  | Per-account check  | `Checking account "name"...`                           |
| `account health`  | Account done (ok)  | `✓ name`                                               |
| `account health`  | Account done (err) | `✗ name — status`                                      |
| `account quota`   | Per-account fetch  | `Fetching quota for "name"...`                         |
| `account quota`   | Account done (ok)  | `✓ name`                                               |
| `account quota`   | Account done (err) | `✗ name — error`                                       |
| `doctor`          | Running checks     | `Running checks...` → `Checking codex binary...` → ... |
| `doctor`          | All passed         | `✓ All checks passed`                                  |
| `doctor`          | Failures           | `✗ N checks failed`                                    |
| `account refresh` | Before login       | `Preparing login for "name"...`                        |
| `account refresh` | After login ok     | `Saving credentials...` → `✓ Credentials refreshed`    |
| `account add`     | Before login       | `Setting up account "name"...`                         |
| `account add`     | After login ok     | `Saving account...` → `✓ Account "name" added`         |

Ensure:

- Consistent verb tense (present participle for in-progress, past for done)
- Account names always quoted in messages
- Success/failure symbols consistent (✓/✗ in color mode, [ok]/[err] in no-color)
- Messages are concise (under 60 characters)

### Step 11: Run full test suite and lints

```bash
just test
just lint
just check
```

All tests must pass, no new warnings.

### Step 12: Update plan index

Mark this round as `done` with today's date in the README.md execution order table.

File: `/workspaces/codex-session/.plan/01-todo/spinner-parallel-async-ux/README.md`

Update the Round 03 row's Status column from `todo` to `done` and fill in the Completed column.

### Step 13: Mark plan complete and move to done

Since this is the final round:

1. Update the README.md header status from `todo` to `done`
2. Move the plan directory:

```bash
mkdir -p .plan/02-done
mv .plan/01-todo/spinner-parallel-async-ux .plan/02-done/spinner-parallel-async-ux
```

## Acceptance Criteria

1. `doctor` command shows a spinner that updates per-check (in TTY text mode)
2. `doctor` spinner is hidden in JSON mode and non-TTY
3. `account refresh` shows spinner before and after login, not during interactive login
4. `account add` shows spinner before and after login, not during interactive login
5. `SpinnerHandle` cleans up on drop (terminal is restored even on early return / error)
6. CTRL-C during spinner operations exits cleanly without leaving terminal artifacts
7. All spinner messages follow the UX table in Step 10
8. New live integration tests pass:
   - Doctor piped output is clean
   - Doctor JSON output is valid JSON
   - Refresh piped mode has no spinner artifacts
   - Health with failing account cleans up spinners
   - Health killed during fetch exits cleanly
9. All existing tests still pass (`just test`)
10. `just lint` and `just check` pass
11. Plan directory moved to `.plan/02-done/spinner-parallel-async-ux/`

## Next Round

Final round. No further rounds.
