# Round 7 — Config Status, Logging, Safety, and Documentation

This file is the **prex input** for Round 7. Pass its contents verbatim to
`/prex -ar` after R6 has landed and `just check` exits 0.

---

## Prerequisite

Rounds 1–6 have shipped. In addition to the R1–R5 foundation:

- `DoctorAccountEntry` now includes `cooldown_active`, `cooldown_reset_at_unix`,
  `cooldown_reason` (R6).
- `--dry-run` shows `account:` and `account-source:` lines (R6).
- `SessionMeta` includes `account` and `account_source` (R6).
- `hint_for()` covers `auth.native`, `account.active.auth`, `account.cooldowns`,
  `session.account` (R6).
- `help_extras.txt` lists `cooldown` in the account subcommand list (R6).
- `run_once()` and `prepare_invocation()` accept `ResolvedAccount` (R6).

`just check` exits 0 on the current branch.

## Goal

Complete the observability picture:

1. Enrich `config status` with auth health summary and cooldown count.
2. Add account context to tracing spans at invocation start.
3. Add safety warnings to `account remove`.
4. Add resolved account to `version` output.
5. Update README.md and DEVELOPMENT.md with multi-account documentation.

## Background (read these before planning)

- `.plan/multi-account-observability/00-overview.md` — north star.
- `.plan/multi-account-observability/16-phase-doctor-dryrun.md` — R6 changes
  (what's already done).
- `src/commands/config_status.rs` — 190 lines. `ConfigStatusView` at lines
  11–25 shows account/account_source but no auth health or cooldown info.
  `build_view()` at lines 65–126 resolves the account but doesn't query the
  registry or cooldown state.
- `src/commands/version.rs` — 43 lines. `VersionView` at lines 10–19 shows
  wrapper_version, child_path, child_version. No account info.
- `src/commands/account/remove.rs` — 21 lines. Calls `registry.remove()`
  immediately with no pre-removal warnings.
- `src/services/account/retry.rs` — `run_with_retry()` at lines 12–136. Logs
  `account = %account` per attempt (line 77) but the entry `pass-through start`
  span in `pass_through.rs:39` has no account context.
- `src/services/account/registry.rs` — `Registry::current()` returns the LRU
  account, `Registry::list()` returns all accounts, `account_dir()` returns
  the account root path.
- `src/services/account/cooldown.rs` — `read()` and `is_active()` for
  cooldown inspection.
- `src/ui/mod.rs` — rendering methods for config_status, version, etc.
- `README.md` — 109 lines. No mention of multi-account, account subcommands,
  `--account` flag, account env vars, or failover.
- `DEVELOPMENT.md` — 315 lines. No mention of account-related commands,
  testing patterns, or module structure.

## Numbered implementation steps

### 1. Add auth health summary to `config status`

**File:** `src/commands/config_status.rs`, struct `ConfigStatusView` at lines
11–25.

Add three fields:

```rust
pub(crate) struct ConfigStatusView {
    // ...existing fields...
    pub(crate) accounts_count: usize,
    pub(crate) active_account_has_auth: bool,
    pub(crate) accounts_in_cooldown: usize,
}
```

In `build_view()` (lines 65–126), after resolving the account, query the
registry and cooldown state:

```rust
let registry = crate::services::account::registry::Registry::from_config(&ctx.config);
let account_list = registry.list().unwrap_or_default();
let accounts_count = account_list.len();
let active_account_has_auth = account_list
    .iter()
    .find(|a| a.id == resolved_account.id)
    .map_or(true, |a| a.has_auth);
let now_unix = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map(|d| d.as_secs())
    .unwrap_or(0);
let accounts_in_cooldown = account_list
    .iter()
    .filter(|a| {
        let root = registry.account_dir(&a.id);
        crate::services::account::cooldown::read(&root)
            .ok()
            .flatten()
            .is_some_and(|cd| crate::services::account::cooldown::is_active(&cd, now_unix))
    })
    .count();
```

Update text rendering in `src/ui/mod.rs` (`write_config_status`) to include:

```
accounts:       3 (0 in cooldown)
active-auth:    true
```

Place these lines after the `codex_home` line and before `session-root`.

**Files:** `src/commands/config_status.rs`, `src/ui/mod.rs`.

### 2. Add account context to retry tracing spans

**File:** `src/services/account/retry.rs`.

In `run_with_retry()`, add `account` and `account_source` fields to the
per-attempt tracing event. After `resolver::resolve()` returns (around line
~77), log:

```rust
tracing::info!(
    op = "retry.attempt",
    attempt,
    max_retries,
    account = %resolved.id,
    account_source = crate::services::account::resolver::source_label(resolved.source),
);
```

If the existing `run_once()` call at line ~77 already logs
`account = %account`, ensure the resolution source is also included. The goal
is that every retry attempt log line contains both the account name and how it
was resolved, for post-hoc log correlation.

**Files:** `src/services/account/retry.rs`.

### 3. Add safety warnings to `account remove`

**File:** `src/commands/account/remove.rs` (currently 21 lines).

Before the `registry.remove()` call, add two checks:

**3a. Warn if removing the active (current) account:**

```rust
if let Ok(Some(current_id)) = registry.current() {
    if current_id == args.name {
        ctx.ui.write_stderr(&format!(
            "warning: '{}' is the current active account; \
after removal, the next invocation will fall back to account resolution defaults\n",
            args.name,
        ))?;
    }
}
```

**3b. Warn if the account has recently-used sessions:**

```rust
let groups_dir = registry.account_dir(&args.name).join("groups");
if let Ok(entries) = std::fs::read_dir(groups_dir.as_std_path()) {
    let now = std::time::SystemTime::now();
    let recent_count = entries
        .flatten()
        .filter(|e| {
            e.metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|mtime| now.duration_since(mtime).ok())
                .is_some_and(|age| age < std::time::Duration::from_secs(24 * 3600))
        })
        .count();
    if recent_count > 0 {
        ctx.ui.write_stderr(&format!(
            "warning: '{}' has {recent_count} session group(s) used in the \
last 24h; removing it may break in-progress Codex sessions\n",
            args.name,
        ))?;
    }
}
```

The warnings are printed to stderr. The remove still proceeds (non-interactive).
Verify `write_stderr` (or equivalent) exists on the UI trait; if not, use the
nearest equivalent stderr writing method. Check how existing warnings are
written (e.g., the `pid-N` group-id fallback warning in
`services/session/group_id.rs`).

**Files:** `src/commands/account/remove.rs`.

### 4. Add resolved account to `version` output

**File:** `src/commands/version.rs`, struct `VersionView` at lines 10–19.

Add two optional fields:

```rust
pub(crate) struct VersionView {
    pub(crate) wrapper_version: String,
    pub(crate) child_path: Option<String>,
    pub(crate) child_version: Option<String>,
    pub(crate) account: Option<String>,
    pub(crate) account_source: Option<String>,
}
```

In `build_view()` (lines 33–42), resolve the account (swallow errors so
`version` always succeeds):

```rust
let resolved = crate::services::account::resolver::resolve(ctx).ok();
VersionView {
    // ...existing fields...
    account: resolved.as_ref().map(|r| r.id.to_string()),
    account_source: resolved.map(|r|
        crate::services::account::resolver::source_label(r.source).to_owned()),
}
```

Update text rendering in `src/ui/mod.rs` (`write_version`). After the child
version line, add:

```rust
if let Some(ref account) = view.account {
    let source = view.account_source.as_deref().unwrap_or("unknown");
    writeln!(out, "account:         {account} (source: {source})")?;
}
```

**Files:** `src/commands/version.rs`, `src/ui/mod.rs`.

### 5. Update README.md with multi-account documentation

**File:** `README.md`.

**5a. Add account commands to the "Usage" section (after line 28):**

```markdown
- `codex-session account add <NAME> [--from-native]`
- `codex-session account list [--format text|json]`
- `codex-session account current`
- `codex-session account use <NAME>`
- `codex-session account remove <NAME>`
- `codex-session account quota [--all] [--live] [--format text|json]`
- `codex-session account cooldown show|clear [--all] [--account NAME]`
```

**5b. Add `--account` and `--max-retries` to global flags (after line 33):**

```markdown
- `--account <NAME|auto>` selects the account for the invocation.
- `--max-retries <N>` enables 429 failover rotation (requires `--account auto`).
- `--group <ID>` overrides the resolved group-id.
```

**5c. Add a new "Multi-Account Management" section after "ConfigRecipe Layout":**

```markdown
## Multi-Account Management

codex-session supports multiple codex accounts, each with isolated auth
and session state:

    codex-session account add work --from-native
    codex-session account add personal --from-native
    codex-session account list
    codex-session account use work
    codex-session --account work exec "hello"
    codex-session --account auto exec "hello"

### Account Layout

    $XDG_STATE_HOME/codex-session/
      accounts/
        <account>/
          auth.json
          cooldown.json
          groups/<group-id>/

### Failover

With `--account auto --max-retries N`, the wrapper rotates to the next
eligible account on 429 detection and records a 5-minute cooldown:

    codex-session --account auto --max-retries 2 exec "task"
    codex-session account cooldown show
    codex-session account cooldown clear --all
```

**5d. Update "Environment" section to include account env vars:**

```markdown
- `CODEX_SESSION_ACCOUNT`: override the resolved account.
- `CODEX_SESSION_GROUP`: override the resolved group-id.
- `CODEX_SESSION_ACCOUNT_REGISTRY_DIR`: custom account registry location.
- `CODEX_SESSION_ACCOUNT_QUOTA_TTL_SECS`: quota cache TTL.
- `CODEX_SESSION_ACCOUNT_WEEKLY_FLOOR`: weekly quota floor for selection.
- `CODEX_SESSION_ACCOUNT_FIVE_HOUR_THRESHOLD`: 5-hour quota threshold.
```

**5e. Update the "ConfigRecipe Layout" section to replace the stale path
(`sessions/<terminal-id>/`) with the current account-based layout.**

**5f. Add exit code `75` for "all accounts exhausted (failover)"** if not
already present.

**Files:** `README.md`.

### 6. Update DEVELOPMENT.md with multi-account development notes

**File:** `DEVELOPMENT.md`.

**6a. Add multi-account examples to the interactive testing section:**

```markdown
# Multi-account operations
cargo run -- account add work --from-native
cargo run -- account list
cargo run -- --account work --dry-run exec "hello"
cargo run -- account cooldown show --format json
cargo run -- doctor --format json | jq '.accounts'
```

**6b. Update the repository layout section** to mention account services:

```markdown
  services/     # business logic: config_recipe merge, session management, account
                # registry, quota reader, selector, cooldown, failover
```

**6c. Mention account-related env vars** in the environment section if one
exists.

**Files:** `DEVELOPMENT.md`.

### 7. Tests

1. **Update config status tests** (`tests/cmd_config_status.rs`): assert the
  new `accounts-count`, `active-account-has-auth`, `accounts-in-cooldown`
  fields in JSON output. Update text and JSON snapshots.

2. **New test: config status shows cooldown count.** Create a `TestEnv`, add
  accounts, write a cooldown to one, run `config status --format json`, assert
  `accounts-in-cooldown: 1`.

3. **New test: account remove warns on active account.** Add an account, set it
  current with `account use`, remove it, assert stderr contains the "current
  active account" warning.

4. **New test: account remove warns on recent sessions.** Add an account, run a
  pass-through (creating a group dir with recent mtime), remove it, assert
  stderr contains "session group(s) used in the last 24h".

5. **Update version tests** (`tests/cmd_version.rs`): assert the JSON includes
  `account` and `account-source` fields. Update snapshots.

6. **No new logging tests.** The tracing changes add structured fields visible
  in log files. Verifiable via manual `RUST_LOG=info` smoke test.

**Files:** `tests/cmd_config_status.rs`, `tests/account_lifecycle.rs` (or
similar), `tests/cmd_version.rs`, snapshot files.

## Files touched (representative)

| File | What changes |
|---|---|
| `src/commands/config_status.rs` | `ConfigStatusView` gains auth/cooldown summary fields |
| `src/commands/version.rs` | `VersionView` gains account fields |
| `src/commands/account/remove.rs` | Safety warnings before removal |
| `src/services/account/retry.rs` | Account + source in per-attempt tracing spans |
| `src/ui/mod.rs` | Text rendering for config_status, version |
| `README.md` | Multi-account documentation |
| `DEVELOPMENT.md` | Account dev notes |
| `tests/` | ~4–6 new tests, ~3 updated tests, snapshot regeneration |

**Net LOC estimate:** ~300–400 (including ~80 LOC of tests, ~100 LOC of docs).

## Done criteria

```sh
just check   # must exit 0
```

Manual smoke:

```sh
# Config status
codex-session config status                      # shows accounts: N, active-auth, cooldowns
codex-session config status --format json | jq '{accounts_count, active_account_has_auth, accounts_in_cooldown}'

# Version
codex-session version                            # shows account line
codex-session version --format json | jq '.account'

# Remove warnings
codex-session account add throwaway --from-native
codex-session account use throwaway
codex-session account remove throwaway 2>&1 | grep "current active account"

# Logging
RUST_LOG=info codex-session exec "echo test" 2>&1 | grep account_source
```

## Out of scope

- Per-account quota display in config status (use `account quota` instead).
- Interactive confirmation on `account remove` (`--force` flag).
- Architecture documentation (`docs/multi-account-architecture.md`).
- Upstream codex fact file updates (`docs/upstream-codex.md`).

## Constraints / project conventions

- Use `just` recipes for verification (`just lint`, `just test`, `just check`).
- No `println!` / `eprintln!` outside `src/ui/`, `src/error.rs`,
  `src/logging.rs`, `tests/` (print-ownership lint).
- Structured logging via `tracing` with `op=` keys.
- `pub(crate)` visibility for all new types.
- Errors use existing `AppError` / `AccountError` enums.
- Snapshots regenerated via `INSTA_UPDATE=always just test-unit` then
  `cargo insta review`.
