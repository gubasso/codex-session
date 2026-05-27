# Round 02: Thread Capture Wiring

> Plan: thread-resume-support | Round: 02 of 04 | Complexity: L
> Generated: 2026-05-26T00:00:00Z | Repo: /workspaces/codex-session

## Context

codex-session wraps OpenAI's Codex CLI with multi-account support by setting
`CODEX_HOME` to a per-account session directory. To enable thread resume
across accounts, codex-session maintains a cross-account JSONL thread index
at `<state_dir>/thread-index.jsonl`.

This round wires the thread index into the exec path: after each
`codex exec --json` run, codex-session parses the JSONL output for thread
start events and records the thread_id → account mapping in the index.

Codex's `--json` flag produces newline-delimited JSON events to stdout. One
of these events is a thread start event that contains the thread_id. When
`--json` is NOT in the child argv (interactive/TUI mode), stdout is not
parseable JSONL and should not be tee'd.

## Previous Rounds

Round 01 created:
- `src/services/session/thread_index.rs` — `ThreadEntry` struct, `append()`,
  `lookup()`, `last_for_group()`, `last_any()` functions.
- Module registered in `src/services/session/mod.rs`.
- The index file path is `<state_dir>/thread-index.jsonl`.

## Scope of This Round

**IN scope:**
- Detect `--json` in the child argv before invoking the child process.
- When `--json` is present, enable stdout capture (tee to parent stdout +
  buffer) for both retry and non-retry paths.
- Parse the captured stdout buffer for Codex thread start events after the
  child exits.
- Write a `ThreadEntry` to the index with the extracted thread_id, current
  account, group_id, and cwd.
- Handle gracefully: missing thread events (non-exec commands), malformed
  JSONL, empty output.

**OUT of scope:**
- Intercepting `exec resume` commands (Round 03).
- Documentation updates (Round 04).

## Current State

### Key Files

- `/workspaces/codex-session/src/commands/pass_through.rs` — the main exec
  path. Key functions:

  `run()` at line 36 dispatches to `run_with_retry()` for the non-dry-run
  path:
  ```rust
  pub(crate) fn run(
      ctx: &crate::context::AppContext,
      argv: &[std::ffi::OsString],
  ) -> Result<i32, crate::error::AppError> {
      // ...
      crate::services::account::retry::run_with_retry(ctx, argv)
  }
  ```

  `run_once()` at line 87 runs a single child attempt. The `capture` parameter
  controls whether stdout/stderr are tee'd or inherited directly:
  ```rust
  pub(crate) fn run_once(
      ctx: &crate::context::AppContext,
      argv: &[std::ffi::OsString],
      resolved: &crate::services::account::resolver::ResolvedAccount,
      session: &SignalSession,
      capture: bool,
  ) -> Result<(i32, Vec<u8>, Vec<u8>), crate::error::AppError>
  ```

  `prepare_invocation()` at line 121 builds the `ChildInvocation` and
  resolves the session directory. The session directory path and account are
  available here.

- `/workspaces/codex-session/src/services/account/retry.rs` — retry loop.
  `run_with_retry()` calls `pass_through::run_once()` for each attempt. The
  `capture` flag is set based on `max_retries > 0`:
  ```rust
  let capture = max_retries > 0;
  let result = crate::commands::pass_through::run_once(
      ctx, argv, &resolved, &signal_session, capture,
  )?;
  let (exit_code, stdout_buf, stderr_buf) = result;
  ```

  The `single_attempt()` function at line 199 passes `capture: false`.

- `/workspaces/codex-session/src/adapters/spawner.rs` — `spawn_and_wait()`
  (no capture, inherits stdio) and `spawn_and_wait_output()` (captures
  stdout+stderr via tee). The tee implementation already exists and works.

- `/workspaces/codex-session/src/services/session/group_id.rs` — `current()`
  resolves the group_id for the current invocation.

- `/workspaces/codex-session/src/services/session/thread_index.rs` — (from
  Round 01) `ThreadEntry`, `append()`, and query functions.

### Existing Patterns

- **Capture vs inherit:** The `run_once()` function already supports both
  modes. When `capture=true`, `spawn_and_wait_output()` tees stdout/stderr
  to the parent AND captures them in buffers. The captured buffers are
  returned to the caller.

- **Post-flight hooks:** After `run_child()`, `pass_through.rs` already runs
  post-flight operations: `sync_group_auth_to_seed()` and `persist_trust()`.
  Thread index writing fits this same pattern.

- **Argv inspection:** The child argv is a `&[OsString]`. To check for
  `--json`, iterate and compare each element.

## Implementation Steps

### Step 1: Add `has_json_flag()` helper

Add a helper function to detect `--json` in the child argv. Place it in
`src/commands/pass_through.rs` (it's argv inspection specific to the
pass-through path):

```rust
fn has_json_flag(argv: &[std::ffi::OsString]) -> bool {
    argv.iter().any(|arg| arg == "--json")
}
```

This checks for the literal `--json` flag. Codex uses this flag to enable
JSONL output mode.

### Step 2: Add `extract_thread_id()` parser

Add a function to parse captured stdout for Codex thread start events.
Place it in `src/services/session/thread_index.rs` (it's thread-index
domain logic):

```rust
pub(crate) fn extract_thread_id(jsonl_output: &[u8]) -> Option<String> {
    // Parse each line as JSON, look for thread start event
    // Codex JSONL events have a "type" field
    // Thread start event has type "thread.started" or similar
    // Return the thread_id field value
}
```

The exact event type and field name must be determined by examining Codex's
`--json` output format. Based on research, look for events with:
- `"type": "thread.created"` or `"type": "thread.started"`
- A `"thread_id"` or `"id"` field containing the UUID

If no thread event is found, return `None` (not all `--json` runs produce
thread events — e.g., `codex exec --json "echo hello"` may not create a
persisted thread).

**Important:** Parse defensively. Unknown event types, missing fields, or
malformed JSON lines should be silently skipped, not cause errors.

### Step 3: Wire capture into `run_once()`

Modify `run_once()` to force `capture=true` when `--json` is in the argv,
regardless of the caller's `capture` parameter. This ensures thread events
are always captured from `--json` runs:

In `run_once()`, after the existing `prepare_invocation()` call, add logic:

```rust
let json_mode = has_json_flag(argv);
let effective_capture = capture || json_mode;
```

Use `effective_capture` instead of `capture` when calling `run_child()`.

### Step 4: Add thread index write to `run_once()` post-flight

After the existing post-flight hooks (`sync_group_auth_to_seed`,
`persist_trust`), add thread index writing when `json_mode` is true and
stdout captured a thread_id:

```rust
if json_mode {
    if let Some(thread_id) = crate::services::session::thread_index::extract_thread_id(&stdout) {
        let entry = crate::services::session::thread_index::ThreadEntry {
            thread_id,
            account: account.to_string(),
            group_id: group_id.clone(),
            cwd: cwd.clone(),
            created_at: /* current timestamp */,
        };
        if let Err(err) = crate::services::session::thread_index::append(
            &ctx.config.paths.state_dir, &entry,
        ) {
            tracing::warn!(
                op = "thread_index.append",
                status = "error",
                err = %err,
            );
        }
    }
}
```

The group_id and cwd are already resolved inside `prepare_invocation()`.
They need to be returned from `prepare_invocation()` or re-resolved. The
cleanest approach is to add `group_id` and `cwd` fields to the
`PreparedInvocation` struct:

```rust
struct PreparedInvocation {
    invocation: ChildInvocation,
    session_dir: camino::Utf8PathBuf,
    baseline_projects: Option<toml::Table>,
    group_id: String,        // NEW
    cwd: Utf8PathBuf,        // NEW
}
```

### Step 5: Update `retry.rs` single_attempt path

`single_attempt()` currently passes `capture: false`. With the new
`effective_capture` logic inside `run_once()`, this is handled
automatically — `run_once()` will upgrade to `capture=true` when `--json`
is detected. No change needed in `retry.rs` itself, but verify that the
`single_attempt()` path correctly returns stdout buffers when capture is
forced.

### Step 6: Add integration test

Create `/workspaces/codex-session/tests/thread_index_capture.rs` (or add
to an existing test file) that:

1. Sets up a mock child binary that outputs JSONL with a thread start event.
2. Runs `codex-session exec --json "prompt"` with the mock.
3. Verifies that `<state_dir>/thread-index.jsonl` contains an entry with the
    correct thread_id and account.

Follow the existing test patterns in `tests/session_dir_persistence.rs` and
`tests/account_passthrough.rs`.

## Acceptance Criteria

- [ ] `has_json_flag()` correctly detects `--json` in child argv.
- [ ] `extract_thread_id()` parses Codex JSONL for thread start events.
- [ ] `extract_thread_id()` returns `None` for non-thread JSONL output.
- [ ] `extract_thread_id()` handles malformed/empty input gracefully.
- [ ] When `--json` is in argv, stdout is captured (tee'd) even for
      single-attempt runs.
- [ ] After a `--json` exec run that produces a thread event, an entry is
      appended to `<state_dir>/thread-index.jsonl`.
- [ ] The thread index entry contains the correct account, group_id, and cwd.
- [ ] Non-`--json` runs are unaffected (no capture overhead).
- [ ] `just test` passes (unit + integration).
- [ ] `just lint` passes.

## Next Round

Round 03 implements resume routing: intercepting `exec resume <ID>` and
`exec resume --last` in the pass-through path, looking up the thread→account
mapping in the cross-account index, and routing the resume request to the
correct account's `CODEX_HOME`.
