# Round 4 — Reactive 429 failover + retry-with-rotation + AuthBridge cleanup

This file is the **prex input** for Round 4. Pass its contents verbatim to `/prex -ar`.

---

## Prerequisite

Rounds 1, 2, 3 have landed. `--account auto` proactively picks accounts based on cached quota + caam scoring. Cooldown files are read by the selector but no code writes them yet.

## Goal

- Add caam-borrowed regex 429 detector in `services/account/failover.rs`.
- Add per-account cooldown writes (`services/account/cooldown.rs`).
- Wrap `pass_through::run` in a retry-with-rotation harness (`services/account/retry.rs`).
- Add `account cooldown {show, clear}` sub-verb tree.
- **Cleanup:** delete `src/services/auth/watcher.rs`, `src/services/auth/signal.rs`, and the AuthBridge `last_refresh` timestamp dance. Slim `src/services/auth.rs` to one function. Drop the R1 `#[ignore]` watcher tests.

## Background (read before planning)

- `.plan/multi-account-refactor/07-failover-spec.md` — full spec (regex patterns, detector placement, cooldown schema, retry-with-rotation harness, CLI surface, test matrix).
- `.plan/multi-account-refactor/04-decisions.md` — ADRs D3 (retire AuthBridge), D6 (caam regex detector).
- `.plan/multi-account-refactor/01-architecture.md` — pre-exec call-flow diagram showing where retry sits.
- caam detector source: `raw.githubusercontent.com/Dicklesworthstone/coding_agent_account_manager/refs/heads/main/internal/ratelimit/detector.go` (cite this URL in `failover.rs` doc comment).
- caam wrapper source: `raw.githubusercontent.com/Dicklesworthstone/coding_agent_account_manager/refs/heads/main/internal/wrap/wrap.go` (retry-loop shape reference).

## Numbered implementation steps

1. **New `src/services/account/failover.rs` (passive observer).**
    - Public surface:
      ```rust
      pub(crate) struct Match {
          pub line_no: usize,
          pub snippet: String,
      }

      pub(crate) fn scan(buf: &[u8]) -> Option<Match>;
      ```
    - Use the `regex` crate (already a transitive dep — verify; otherwise add it directly). Pattern verbatim from caam:
      ```rust
      static PATTERN: Lazy<Regex> = Lazy::new(|| {
          Regex::new(r"(?i)\b(429|rate[- ]limit|too many requests|quota exceeded|slow down)\b").unwrap()
      });
      ```
    - `scan` walks the buffer line by line (UTF-8-lossy via `bstr` or manual), returns the first match.
    - **Tee strategy: post-wait scan**, not mid-flight streaming. Document in the module doc comment why (per `07-failover-spec.md` rationale).

2. **New `src/services/account/cooldown.rs`.**
    - Schema (per `07-failover-spec.md`):
      ```rust
      #[derive(Serialize, Deserialize, Debug, PartialEq)]
      pub(crate) struct Cooldown {
          pub reset_at_unix: u64,
          pub reason: String,
          pub last_429_at_unix: u64,
          pub snippet_truncated: String,
      }
      ```
    - Public surface:
      ```rust
      pub(crate) fn path(state_root: &Utf8Path, account: &AccountId) -> Utf8PathBuf;
      pub(crate) fn read(state_root: &Utf8Path, account: &AccountId) -> Result<Option<Cooldown>, CooldownError>;
      pub(crate) fn write(state_root: &Utf8Path, account: &AccountId, cooldown: &Cooldown) -> Result<(), CooldownError>;
      pub(crate) fn clear(state_root: &Utf8Path, account: &AccountId) -> Result<(), CooldownError>;
      pub(crate) fn clear_all(state_root: &Utf8Path) -> Result<usize, CooldownError>;
      pub(crate) fn is_active(cd: &Cooldown, now_unix: u64) -> bool { cd.reset_at_unix > now_unix }
      ```
    - Atomic write via `src/adapters/fs.rs`.
    - `selector.rs` (R3) already reads via `cooldown::read` — verify the call site uses this new module (R3 may have inlined; refactor if so).

3. **New `src/services/account/retry.rs` (the harness).**
    - Public surface:
      ```rust
      pub(crate) fn run_with_retry(ctx: &AppContext, argv: &[OsString]) -> Result<i32, AppError>;
      ```
    - Body:
      ```rust
      let max_retries = ctx.global.max_retries;
      for attempt in 0..=max_retries {
          let account = resolver::resolve(ctx)?;
          let result = pass_through::run_once(ctx, argv, &account);
          match result {
              Ok((exit_code, captured)) if attempt < max_retries => {
                  if let Some(m) = failover::scan(&captured) {
                      let cd = Cooldown {
                          reset_at_unix: now_unix() + 300,
                          reason: format!("429 detected: {:?}", m.snippet),
                          last_429_at_unix: now_unix(),
                          snippet_truncated: m.snippet.chars().take(256).collect(),
                      };
                      cooldown::write(state_root, &account, &cd)?;
                      tracing::warn!(op = "account.switch", from = %account, attempt, reason = "429");
                      continue;
                  }
                  return Ok(exit_code);
              }
              Ok((exit_code, _)) => return Ok(exit_code),
              Err(e) => return Err(e),
          }
      }
      Err(AppError::Account(AccountError::NoEligible))
      ```
    - **Warning case:** if `max_retries > 0` AND the resolved account is explicitly pinned (not `auto`), emit a warning at the start: "retries with pinned account are no-ops; use --account auto for failover".

4. **Refactor `src/commands/pass_through.rs`.**
    - Split `run(ctx, argv)` into:
      - `pub(crate) fn run(ctx, argv) -> Result<i32, AppError>` — outer entry; delegates to `retry::run_with_retry(ctx, argv)`.
      - `pub(crate) fn run_once(ctx, argv, account: &AccountId) -> Result<(i32, Vec<u8>), AppError>` — existing body, but:
        - Account-id parameter replaces the in-function `resolver::resolve` call.
        - Spawn captures stdout + stderr with `Stdio::piped()` (changed from default `inherit()`).
        - After `wait_with_output()`, forward the captured bytes to parent's stdout/stderr unmodified (use `io::stdout().write_all(&output.stdout)?` etc — these are in `src/ui/` whitelist territory? Actually `pass_through.rs` is in `src/commands/`. **Solution:** add a thin pass-through helper in `src/ui/` that takes raw bytes and writes; OR exempt this specific call from the print-ownership lint via a `// LINT-EXCEPTION` comment + justification; OR the cleanest: write to `io::stderr()` / `io::stdout()` directly in `commands/` is forbidden by the lint, so create `src/ui/raw_passthrough.rs` with a small function `write_raw(stream: WhichStream, bytes: &[u8])`.
        - Return both exit code and the captured combined buffer.

5. **Add `src/commands/account/cooldown.rs` (or `cooldown/` subdir).**
    - Choose: if `show` and `clear` are short (< 60 LOC each), a single file `commands/account/cooldown.rs` suffices. If they grow, split into `commands/account/cooldown/{mod,show,clear}.rs`. Start single-file.
    - Register: `AccountSubcommand::Cooldown(CooldownArgs)` in `src/cli/account.rs`. `CooldownArgs` has its own `#[command(subcommand)] sub: CooldownSubcommand { Show(ShowArgs), Clear(ClearArgs) }`.
    - `show`: lists cooldowns across all accounts (default), with `--account <name>` for one, `--json` for structured.
    - `clear`: requires `--account <name>` or `--all`; error with `EX_USAGE` if neither.
    - Outputs per `07-failover-spec.md`.

6. **Add `--max-retries <N>` global flag.**
    - In `src/cli/mod.rs::GlobalArgs`, add `pub(crate) max_retries: u32` (default 0).
    - No env mirror needed (it's a per-invocation knob).

7. **Cleanup: delete the AuthBridge watcher + signal modules.**
    - `git rm src/services/auth/watcher.rs src/services/auth/signal.rs`.
    - In `src/services/auth.rs`:
      - Remove the `AuthBridge` struct.
      - Remove `seed_into_session`, `persist_to_native`, `sync_once`, `last_refresh_from_json`, and the `last_refresh` timestamp comparison.
      - Keep only `pub(crate) fn import_if_missing(group_dir, native_home) -> Result<(), AuthError>` (introduced in R1) and the helper `secure_file_write_atomic` if not already in `src/adapters/fs.rs`.
      - Slim `AuthError` to just `Io`, `BadOwnership`, `SymlinkRefused`. Remove `LockFailed`, `JsonParse`, etc., if no longer raised. (Verify by `cargo check` — clippy will flag dead variants.)
    - In `src/services/auth/mod.rs` (if exists), remove `pub(crate) mod watcher` / `signal` declarations.
    - **Signal forwarding:** if `signal.rs` was the only signal-forwarding code (forwarding SIGINT/SIGTERM/SIGWINCH to the child), that's a project-functionality regression. Check `src/adapters/spawner.rs` and `src/commands/pass_through.rs`: does the existing spawn already forward signals via `Command`'s default behavior + process group, or does it rely on `auth/signal.rs::install`? If the latter, MOVE the signal-forwarding code to `src/adapters/spawner.rs` or `src/commands/pass_through.rs` first as a separate step, THEN delete `auth/signal.rs`.
    - Delete `tests/auth_bridge_seed_and_persist.rs::watcher_propagates_refresh_back_into_running_session` (was `#[ignore]`'d in R1).
    - Delete other watcher-specific tests; keep the seed-and-persist tests that match the new one-shot semantics.

8. **Tests.**
    - **Detector unit tests** in `src/services/account/failover.rs::tests`: table-driven per the matrix in `07-failover-spec.md` (8 positives, 3 negatives).
    - **Cooldown round-trip** in `src/services/account/cooldown.rs::tests`: `write` then `read` returns the same struct; `is_active` works; `clear_all` returns the count removed.
    - **End-to-end retry** in `tests/account_failover_retry.rs`:
      - Create fixture `tests/fixtures/fake-429.sh` that prints `HTTP 429 Too Many Requests` to stderr and exits 1.
      - Register two accounts.
      - Invoke `codex-session --account auto --max-retries 2 exec "trigger 429"` with `CODEX_SESSION_CHILD_BIN=tests/fixtures/fake-429.sh`.
      - Assert: second invocation in the retry loop used a different account; cooldown.json written for the first; if both hit 429 → third try returns `NoEligible` (exit 75).
    - **Pinned-account warning** in `tests/account_failover_pinned.rs`: `--account work --max-retries 2 exec ...` emits the warning.
    - **CLI verb** in `tests/account_cooldown_cli.rs`: `show` text + JSON output; `clear --all`; `clear --account <name>`; `clear` without args errors with 64.
    - **Snapshot updates**: `tests/cmd_root_help.rs`, `tests/cmd_account_help.rs` (if exists) reflect the new `cooldown` sub-verb.

## Files touched (representative)

- `src/services/account/{failover,retry,cooldown}.rs` (NEW)
- `src/services/account/selector.rs` (verify it uses `cooldown::read` — refactor if R3 inlined)
- `src/services/account/mod.rs` (re-exports)
- `src/commands/pass_through.rs` (split run / run_once; pipe stdio)
- `src/commands/account/cooldown.rs` (NEW)
- `src/cli/account.rs` (register Cooldown sub-verb)
- `src/cli/mod.rs` (GlobalArgs gains `--max-retries`)
- `src/services/auth.rs` (slim down)
- `src/services/auth/watcher.rs`, `src/services/auth/signal.rs` (**DELETE** — but move signal-forwarding first if it's load-bearing)
- `src/ui/raw_passthrough.rs` (NEW — minimal, for the stdio tee) OR amend the print-ownership lint exclusions
- `src/adapters/spawner.rs` (possibly accept piped stdio config; possibly host signal-forwarding moved from `auth/signal.rs`)
- `tests/account_failover_*.rs`, `tests/account_cooldown_*.rs` (NEW)
- `tests/auth_bridge_seed_and_persist.rs` (slim — delete watcher-specific cases)
- `tests/fixtures/fake-429.sh` (NEW)

**Net LOC estimate:** ~500–650 (incl. ~250 LOC of *deletions*). **New tests:** ~10–12.

## Done criteria

```sh
just precommit-all     # must exit 0
```

Plus manual smoke:

```sh
# With a real codex install that returns 429 (or use the fake fixture):
CODEX_SESSION_CHILD_BIN=$(realpath tests/fixtures/fake-429.sh) \
  codex-session --account auto --max-retries 2 exec "trigger 429"
# Expected: rotates to second account; if both 429, exits 75.

codex-session account cooldown show
codex-session account cooldown show --json | jq .
codex-session account cooldown clear --all

# Pinned-account warning
codex-session --account work --max-retries 2 exec "hi" 2>&1 | grep "no-ops"

# Verify the AuthBridge cleanup didn't break the one-shot import
rm ~/.local/state/codex-session/accounts/default/groups/<gid>/auth.json
codex-session exec "hi"   # should re-import from ~/.codex/auth.json
```

## Out of scope for Round 4

- Mid-session rotation (loopback proxy à la `ndycode`).
- Streaming detection (mid-flight, not post-wait) — augment with `PipingSpawner` only if a use case appears.
- Per-account telemetry / TUI dashboard.
- Per-model quota tracking.
- Health-tracking auto-update (in R3 health defaults to "healthy" for everyone; R4 doesn't change this).

## Constraints

- Use `just` recipes for verification.
- caam regex pattern is **verbatim**; cite the source URL in the module doc comment.
- No `eprintln!` outside the print-ownership whitelist. Pass-through of captured child output goes via `src/ui/raw_passthrough.rs` (new minimal module).
- `CooldownError`, `RetryError` use `thiserror`; surface via `AppError::Account(AccountError::...)`.
- New structured-log `op=` keys: `failover.match`, `account.switch`, `cooldown.write`, `cooldown.clear`, `retry.exhausted`.
- Atomic writes for cooldown.json via existing `src/adapters/fs.rs`.
- Detector is a pure function on bytes → easy unit tests. No I/O in `failover::scan`.
- Don't break any existing snapshot tests; update them where the output legitimately changes.
- Keep AuthBridge deletion to *one* commit so it's easy to revert if it regresses signal handling.
