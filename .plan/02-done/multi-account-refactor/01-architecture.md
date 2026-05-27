# 01 — Architecture

## Current (pre-refactor)

```
$XDG_RUNTIME_DIR/codex-session/sessions/<terminal_id>/   ← ephemeral CODEX_HOME
  config.toml, auth.json, sessions/, state_5.sqlite, ...
                ↑
        terminal_id = tty(1) or "pid-{getpid()}"  ← silent fallback (the bug)
```

Key code:

- `src/services/session/terminal_id.rs:7-9` — `current()` shells out to `tty(1)`; on failure, returns `format!("pid-{}", std::process::id())` with no warning.
- `src/services/session/dir.rs:53-62` — `session_dir(root, terminal_id)` joins `<root>/sessions/<terminal_id>/`.
- `src/services/session/dir.rs:200-230` — `secure_dir()` is `mkdir -p + chmod 0700 + uid check`; never wipes.
- `src/commands/pass_through.rs:86-89` — exports `CODEX_HOME = session_dir` for the child.
- `src/services/auth.rs` (`AuthBridge`) — seeds native `~/.codex/auth.json` into the ephemeral dir at start, runs a watcher thread mid-flight, persists back on drop.

Failure mode: `XDG_RUNTIME_DIR` is on tmpfs and wiped on logout; `terminal_id` degenerates to a unique `pid-{pid}` per headless call. Both make cross-invocation resume impossible.

## Target (post-refactor)

```
$XDG_STATE_HOME/codex-session/
  accounts/
    <account>/                       ← "default" until R2; multi from R2 on
      groups/
        <group-id>/                  ← persistent CODEX_HOME
          auth.json                  ← per-account credential
          config.toml                ← composed from config_recipe layers (existing)
          sessions/                  ← native codex rollouts; resume works
          state_5.sqlite             ← native codex state DB
          session_index.jsonl
          memories/, plugins/, ...   ← native codex global state
          runs/<run-id>/             ← optional ephemeral per-invocation scratch
      cooldown.json                  ← {reset_at_unix, reason, last_429_at_unix}
  cache/quota/<account>.json         ← TTL ~30s, parsed wham/usage payload
  state/last-account                 ← LRU pointer for selector
```

## Group-id resolution chain

`services/session/group_id.rs::current()` walks, in order, and returns at first hit:

1. **`--group <id>` flag** — explicit CLI override. Validated against `[a-z0-9][a-z0-9_-]{0,31}`.
2. **`CODEX_SESSION_GROUP` env var** — set by parent (e.g., a Claude-Code session can export this once at startup).
3. **Stable TTY id** — `tty(1)` → strip `/dev/` → replace `/` with `-` → e.g., `pts-0`.
4. **`PPID + boot_id` composite** — `format!("ppid-{ppid}-{starttime}")` from `/proc/<ppid>/stat` field 22. Stable across parent re-exec.
5. **`pid-{pid}` fallback** — **emits stderr warning**:
    ```
    warning: codex-session group-id falling back to pid-{N}; resume will not persist
            across invocations. Set CODEX_SESSION_GROUP=<id> for stable resume.
    ```
    This is the *only* fallback that breaks resume — surfacing it loudly prevents the silent-failure mode that started this whole refactor.

The chosen value is recorded on `SessionContext { group_id, group_id_source: GroupIdSource }` so `doctor`, `config status`, and structured logs can report which arm fired.

## Account resolution chain

`services/account/resolver.rs::resolve()` walks:

1. **`--account <name>` flag** — explicit; `"auto"` is a sentinel that triggers the selector (R3+).
2. **`CODEX_SESSION_ACCOUNT` env var** — same shape as the flag.
3. **`Config.account.pinned`** — from `[account] pinned = "..."` in config files. Written by `codex-session account use <name>`.
4. **`Config.account.default`** — from `[account] default = "..."`.
5. **`"default"` literal** — always works; the default account is auto-created on first run.

When the resolver returns `"auto"`, the caller invokes `selector::pick()` (R3+). Until R3, `auto` warns and falls through to `pinned`/`default`.

## AuthBridge demotion (R1) and removal (R4)

**Current `AuthBridge` responsibilities:**

- `seed_into_session()` — copy `~/.codex/auth.json` into the ephemeral CODEX_HOME at start.
- `watcher::run_until()` — bg thread polling native `auth.json`; on mid-flight token refresh, copy the new auth into the session dir.
- `persist_to_native()` — on drop, copy the (possibly token-refreshed) session `auth.json` back to native, with a `last_refresh` timestamp guard to prevent stale rollback.

**R1 (demote):**

- Keep only `import_if_missing(group_dir, native_home)`: if `<group-dir>/auth.json` already exists, no-op; else copy native once via `secure_file_write_atomic`.
- Unwire watcher + persist-on-drop from `pass_through.rs`. Modules `auth/watcher.rs`, `auth/signal.rs` stay compiled but unused.
- Rationale: each account-group now owns its own persistent `auth.json`; codex itself handles token refresh in place; no cross-CODEX_HOME copy needed.

**R4 (delete):**

- Delete `src/services/auth/watcher.rs` and `src/services/auth/signal.rs`.
- Slim `src/services/auth.rs` to the single `import_if_missing` function + the `secure_file_write_atomic` / ownership-validation helpers.
- Delete watcher-specific tests in `tests/auth_bridge_*.rs`.
- ~250 LOC removed total.

## Legacy-dir pruning (R1)

`services/session/cleanup.rs` (existing 7-day-TTL pruner) gets a new pass:

- Scan `<XDG_RUNTIME_DIR>/codex-session/sessions/pid-*/` and `<XDG_STATE_HOME>/codex-session/sessions/pid-*/`.
- Remove entries with mtime > 24 h.
- Log-and-swallow on errors. Best-effort.

Rationale: pre-refactor users accumulate one `pid-{pid}/` dir per invocation. Cleanup is a one-time courtesy after upgrading.

## Pre-exec call flow (post-refactor)

```
pass_through::run(ctx, argv)
  ├─ retry::run(ctx, argv)                    ← R4: outer retry loop
  │   ├─ pass_through::run_once(ctx, argv, account_id)
  │   │   ├─ account_id = resolver::resolve(ctx, --account/env/pinned/default/"auto")
  │   │   │      └─ if "auto" → selector::pick(ctx)             ← R3
  │   │   ├─ group_id = group_id::current(ctx, --group/env/tty/ppid/pid-N+warn)
  │   │   ├─ session_dir = state_root/accounts/<account_id>/groups/<group_id>/
  │   │   │      └─ secure_dir(session_dir)
  │   │   ├─ config-recipe::compose(recipe_name, paths) [if --config-recipe]
  │   │   ├─ config-recipe::write_session_artifacts(composition, session_dir)
  │   │   ├─ session::meta::write(session_dir, meta)
  │   │   ├─ AuthBridge::import_if_missing(session_dir, native_home)
  │   │   ├─ child_env = ChildEnv::scrubbed_default()
  │   │   │      .with("CODEX_HOME" = session_dir)
  │   │   │      └─ scrubs all CODEX_SESSION_* from child env
  │   │   ├─ spawner.spawn_and_wait(inv)
  │   │   │      └─ failover::observe(stdout, stderr) → match? mark cooldown   ← R4
  │   │   └─ trust_sync::persist_trust(session_config, cache_settings, baseline)
  │   └─ if 429 detected and --max-retries > 0:
  │         cooldown::write(account_id, reset_at)
  │         → retry with selector::pick() excluding cooldown'd accounts
```

## Module map (post-refactor)

```
src/
├── cli/
│   ├── mod.rs              ← + Commands::Account variant, + GlobalArgs flags
│   ├── account.rs (NEW)    ← AccountArgs + AccountSubcommand
│   └── ... (existing)
├── commands/
│   ├── account/ (NEW)
│   │   ├── mod.rs          ← dispatch by sub-verb
│   │   ├── add.rs, list.rs, current.rs, use_.rs, remove.rs   ← R2
│   │   ├── quota.rs                                           ← R3
│   │   └── cooldown.rs (or cooldown/ subdir)                  ← R4
│   ├── dispatch.rs         ← + Account arm
│   ├── pass_through.rs     ← split run / run_once             ← R4
│   └── ... (existing)
├── services/
│   ├── account/ (NEW)
│   │   ├── mod.rs
│   │   ├── registry.rs     ← R2: filesystem-backed account store
│   │   ├── resolver.rs     ← R2 (auto arm in R3)
│   │   ├── quota.rs        ← R3: wham/usage HTTP client + cache
│   │   ├── selector.rs     ← R3: caam scoring formula
│   │   ├── failover.rs     ← R4: regex 429 detector
│   │   ├── retry.rs        ← R4: retry-with-rotation harness
│   │   └── cooldown.rs     ← R4: per-account cooldown schema
│   ├── auth.rs             ← R1 demote; R4 slim to one fn
│   ├── auth/
│   │   ├── watcher.rs      ← R4 DELETE
│   │   └── signal.rs       ← R4 DELETE
│   ├── session/
│   │   ├── dir.rs          ← R1 path layout change
│   │   ├── group_id.rs (NEW) ← R1, replaces terminal_id.rs
│   │   ├── cleanup.rs      ← R1 + legacy-dir prune
│   │   └── ... (existing)
│   └── ... (existing)
├── config/
│   └── mod.rs              ← R2 + [account] section
├── ui/
│   └── help_extras.txt (NEW or extended) ← R2
└── ... (existing)
```

## What stays unchanged

- `src/adapters/fs.rs` — atomic-rename helper (used by cooldown.json, quota cache)
- `src/adapters/spawner.rs` — fork/exec/wait (R4 keeps the same Spawner; failover observes post-wait)
- `src/services/config_recipe/` — config_recipe composition engine
- `src/services/trust_sync.rs` — post-flight trust diff
- `src/ui/` — rendering layer
- `src/logging.rs` — tracing setup; new `op=` keys added (`account.select`, `account.switch`, `quota.fetch`, `failover.match`, `group_id.fallback`)
- `src/error.rs` — same enum shape; new `AppError::Account(AccountError)` variant added with `.kind()` + `.exit_code()`
