# Round 01: Thread Index Data Layer

> Plan: thread-resume-support | Round: 01 of 04 | Complexity: L
> Generated: 2026-05-26T00:00:00Z | Repo: /workspaces/codex-session

## Context

codex-session wraps OpenAI's Codex CLI with multi-account support by setting
`CODEX_HOME` to a per-account session directory for each invocation. Codex
stores all session state (threads, rollouts, `state_5.sqlite`) under
`$CODEX_HOME`, so each account's threads are physically isolated.

When a caller uses `codex-session exec resume <SESSION_ID>` and
`--account auto` selects a different account than the one that created the
thread, the resume fails because the target account's `CODEX_HOME` has no
record of that thread.

To solve this, codex-session needs a cross-account thread index: a data
structure that maps thread IDs to the account that created them, enabling
smart routing of resume requests. This round builds the data layer for that
index.

## Previous Rounds

This is the first round — no prior rounds.

## Scope of This Round

**IN scope:**
- New `src/services/session/thread_index.rs` module with types and I/O for
  the JSONL thread index.
- `ThreadEntry` struct: thread_id, account, group_id, cwd, created_at.
- `append()` function: append one entry to the index file.
- `last_for_group()` function: scan the index and return the most recent
  entry matching a group_id.
- `last_any()` function: scan the index and return the most recent entry
  regardless of group.
- `lookup()` function: find the entry matching a specific thread_id.
- Unit tests for all functions.
- Register the module in `src/services/session/mod.rs`.

**OUT of scope:**
- Wiring the index into the pass-through exec path (Round 02).
- Resume routing logic (Round 03).
- Documentation updates (Round 04).

## Current State

### Key Files

- `/workspaces/codex-session/src/services/session/mod.rs` — session service
  module root. Currently exposes `cleanup`, `dir`, `group_id`, `meta`
  submodules.

- `/workspaces/codex-session/src/services/session/meta.rs` — `SessionMeta`
  struct and `write()` function for session metadata. Uses `tempfile +
  persist` atomic write pattern, `serde::Serialize`, and `serde_json`. Good
  reference for the project's file I/O conventions:

  ```rust
  #[derive(Debug, Serialize)]
  #[serde(rename_all = "kebab-case")]
  pub(crate) struct SessionMeta<'a> {
      pub(crate) profile: Option<&'a str>,
      pub(crate) group_id: &'a str,
      pub(crate) cwd: &'a Utf8Path,
      pub(crate) started_at: String,
      pub(crate) account: &'a str,
      pub(crate) account_source: &'a str,
  }
  ```

- `/workspaces/codex-session/src/services/session/dir.rs` — session directory
  resolution. The session root is resolved from `state_dir` (preferred) or
  `runtime_dir`. The thread index file should live at the session root level
  (not inside any account subdirectory), since it's a cross-account resource.
  Key function:

  ```rust
  pub(crate) fn resolve_session_root(
      runtime_dir: Option<&Utf8Path>,
      state_dir: &Utf8Path,
  ) -> Result<SessionRoot, crate::config::ConfigError>
  ```

- `/workspaces/codex-session/src/services/session/group_id.rs` — `GroupId`
  type with validation. Thread index entries reference group IDs.

- `/workspaces/codex-session/src/services/account/id.rs` — `AccountId` type.
  Thread index entries reference account IDs.

- `/workspaces/codex-session/src/config/mod.rs` — `PathsConfig` with
  `state_dir`, `cache_dir`, `runtime_dir` fields. The thread index file lives
  at `<state_dir>/thread-index.jsonl`.

### Existing Patterns

- **serde_json for serialization** — all JSON I/O uses `serde_json`.
- **`camino::Utf8PathBuf`** for all path types.
- **`#[serde(rename_all = "kebab-case")]`** on serializable structs.
- **Error types** use `crate::config::ConfigError` for I/O errors.
- **`crate::adapters::fs::atomic_write`** for atomic file writes — but this
  index uses append-only JSONL, so a simple `OpenOptions::append()` suffices.
- **Module visibility** — all items are `pub(crate)`.
- **Test patterns** — tests use `tempfile::tempdir()` and build paths from
  the temp base.

## Implementation Steps

### Step 1: Create `src/services/session/thread_index.rs`

Create the new module with the `ThreadEntry` struct and serialization:

```rust
use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct ThreadEntry {
    pub(crate) thread_id: String,
    pub(crate) account: String,
    pub(crate) group_id: String,
    pub(crate) cwd: Utf8PathBuf,
    pub(crate) created_at: String,
}
```

The `thread_id` is stored as an opaque string (Codex uses UUID v7 but we
don't validate the format — future-proof).

### Step 2: Implement `index_path()`

Helper to resolve the index file path:

```rust
pub(crate) fn index_path(state_dir: &Utf8Path) -> Utf8PathBuf {
    state_dir.join("thread-index.jsonl")
}
```

The index lives at `<state_dir>/thread-index.jsonl`, outside any account
subdirectory, because it's a cross-account resource.

### Step 3: Implement `append()`

Append one `ThreadEntry` as a single JSON line. Use `OpenOptions` with
`create(true).append(true)` for crash-safe append-only writes:

```rust
pub(crate) fn append(
    state_dir: &Utf8Path,
    entry: &ThreadEntry,
) -> Result<(), std::io::Error> {
    let path = index_path(state_dir);
    // ... serialize to JSON line, append to file
}
```

Ensure the serialized JSON contains no embedded newlines (serde_json's
default compact format guarantees this).

### Step 4: Implement `lookup()`

Scan the JSONL file for a specific thread_id. Return `Option<ThreadEntry>`.
If the file doesn't exist, return `None` (no error — first run has no index).

```rust
pub(crate) fn lookup(
    state_dir: &Utf8Path,
    thread_id: &str,
) -> Result<Option<ThreadEntry>, std::io::Error>
```

Read the file line by line, deserialize each line, return the last match
(in case of duplicates from re-runs, the most recent entry wins).

### Step 5: Implement `last_for_group()`

Scan the JSONL file and return the most recent entry matching a given
`group_id`. "Most recent" = last matching line in the file (append-only
means chronological order).

```rust
pub(crate) fn last_for_group(
    state_dir: &Utf8Path,
    group_id: &str,
) -> Result<Option<ThreadEntry>, std::io::Error>
```

### Step 6: Implement `last_any()`

Return the last entry in the file regardless of group. Used for
`--all-groups` flag.

```rust
pub(crate) fn last_any(
    state_dir: &Utf8Path,
) -> Result<Option<ThreadEntry>, std::io::Error>
```

### Step 7: Register the module

Add `pub(crate) mod thread_index;` to
`/workspaces/codex-session/src/services/session/mod.rs`.

### Step 8: Write unit tests

Test in a `#[cfg(test)] mod tests` block inside `thread_index.rs`:

1. `append` creates the file and writes a valid JSON line.
2. `append` appends to an existing file without overwriting.
3. `lookup` finds an entry by thread_id.
4. `lookup` returns `None` for unknown thread_id.
5. `lookup` returns `None` when file doesn't exist.
6. `last_for_group` returns the most recent entry for the group.
7. `last_for_group` returns `None` for unknown group.
8. `last_any` returns the last entry regardless of group.
9. Malformed lines in the JSONL file are skipped (resilience).

Use `tempfile::tempdir()` for all test I/O.

## Acceptance Criteria

- [ ] `src/services/session/thread_index.rs` compiles and is registered in
      `mod.rs`.
- [ ] `ThreadEntry` struct with serde derives is defined.
- [ ] `append()` creates and appends to `<state_dir>/thread-index.jsonl`.
- [ ] `lookup()` finds entries by thread_id, returns `None` on miss or
      missing file.
- [ ] `last_for_group()` returns the most recent entry for a group_id.
- [ ] `last_any()` returns the most recent entry globally.
- [ ] All unit tests pass via `just test-unit`.
- [ ] `just lint` passes (fmt + clippy-strict + print-ownership).

## Next Round

Round 02 wires the thread index into the pass-through exec path: detecting
`--json` in the child argv, tee'ing stdout to capture JSONL events, parsing
thread start events, and writing entries to the index after each exec run.
