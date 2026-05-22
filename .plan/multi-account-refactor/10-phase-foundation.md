# Round 1 — Foundation: persistent CODEX_HOME + group-id fallback (bug fix)

This file is the **prex input** for Round 1. Pass its contents verbatim to `/prex -ar`.

---

## Goal

Replace `codex-session`'s ephemeral per-call `CODEX_HOME` with a persistent `accounts/default/groups/<group-id>/` layout under `$XDG_STATE_HOME/codex-session/`. Fix the silent `pid-N` fallback. Demote `AuthBridge` to a one-shot importer. Prune legacy `pid-*` dirs on first run.

This round is **strictly bug-fix + path layout**. No multi-account CLI work — the `<account>` slot stays hardcoded to `"default"` until Round 2.

## Background (read these before planning)

- `.plan/multi-account-refactor/00-overview.md` — north star, glossary.
- `.plan/multi-account-refactor/01-architecture.md` — current vs target layout, group-id resolution chain, AuthBridge demotion details.
- `.plan/multi-account-refactor/02-references.md` — upstream codex SoT (CODEX_HOME semantics F1, resume contract F3, write-back race F6).
- `.plan/multi-account-refactor/04-decisions.md` — ADRs D1 (persistent CODEX_HOME), D2 (two-axis layout), D3 (retire AuthBridge).
- `.plan/multi-account-refactor/05-cli-design.md` — global flag conventions, env-scrubbing rule.
- `docs/upstream-codex.md` — F6 (CODEX_HOME), F7 (write-back model).
- `CLAUDE.md` — quality-gate recipes (use `just`, not raw `cargo`).

## Numbered implementation steps

1. **Rename and rewrite `src/services/session/terminal_id.rs` → `src/services/session/group_id.rs`.**
    - New `current(ctx)` function walks the 5-step resolution chain:
      1. `--group <id>` flag (from `cli::GlobalArgs::group`).
      2. `CODEX_SESSION_GROUP` env var.
      3. Stable TTY id (`tty(1)` → strip `/dev/` → replace `/` with `-`).
      4. `PPID + /proc/<ppid>/stat field 22 (starttime)` → `ppid-{ppid}-{starttime}`. Use the `procfs` crate or read+parse manually. Stable across parent re-exec.
      5. `pid-{getpid()}` — and **emit a stderr warning** via `ctx.ui` (NOT `eprintln!` directly — `ctx.ui` exists for this; the print-ownership lint forbids `eprintln!` outside `src/ui/`/`error.rs`/`logging.rs`/`tests/`). Warning text: `warning: codex-session group-id falling back to pid-{N}; resume will not persist across invocations. Set CODEX_SESSION_GROUP=<id> for stable resume.`
    - Add a `GroupIdSource` enum (`Flag | Env | Tty | Ppid | Pid`) returned alongside the id so `doctor`, `config status`, and structured logging can report which arm fired.
    - Add `GroupId` newtype wrapping `String`, with `FromStr` validating against `[a-z0-9][a-z0-9_-]{0,31}` and a 64-char hard cap. Reject invalid env / flag values with `EX_USAGE` (64).
    - Update `src/services/session/mod.rs` exports.

2. **Update `src/services/session/dir.rs::session_dir`.**
    - New signature: `pub(crate) fn session_dir(root: &Utf8Path, account: &str, group_id: &str) -> Result<Utf8PathBuf, ConfigError>`.
    - New path: `<root>/accounts/<account>/groups/<group_id>/`.
    - For Round 1, callers pass `account = "default"` (a literal). Round 2 wires the resolver.
    - **Switch root preference** from `XDG_RUNTIME_DIR` (tmpfs, wiped on logout) to `XDG_STATE_HOME` (persistent). Update `resolve_session_root` so `XDG_STATE_HOME` is preferred; keep `XDG_RUNTIME_DIR` as a fallback only if state is unavailable.
    - Keep `secure_dir` validation unchanged — call it on each path segment we mkdir.
    - Update `inspect_session_root` (the read-only variant) to reflect the new layout so `doctor` doesn't choke.

3. **Add `--group <id>` global flag.**
    - In `src/cli/mod.rs::GlobalArgs`, add `pub(crate) group: Option<GroupId>`.
    - Long-form only (per CLI design): `--group`. No short flag.
    - Wire the value into `AppContext` or pass through to `group_id::current`.

4. **Demote `AuthBridge` to one-shot importer.**
    - In `src/services/auth.rs`, add `pub(crate) fn import_if_missing(group_dir: &Utf8Path, native_home: &Utf8Path) -> Result<(), AuthError>`. If `<group-dir>/auth.json` exists → `Ok(())`. Else copy native `auth.json` once via `secure_file_write_atomic`.
    - Keep `AuthBridge` struct and its existing methods compiled for now (R4 deletes them).
    - In `src/commands/pass_through.rs`: replace `AuthBridge::new() + bridge.seed_into_session()` + watcher setup + `bridge.persist_to_native()` on drop with a single call to `auth::import_if_missing(session_dir, native_home)` *before* the spawn. Remove all `auth::watcher::*` and `auth::signal::*` wiring from `pass_through.rs` (the modules stay compiled but unused).
    - Update any tests that rely on the watcher to either skip or assert the new one-shot semantics. Two of the four tests in `tests/auth_bridge_seed_and_persist.rs` will need to change shape; the watcher-specific test (`watcher_propagates_refresh_back_into_running_session`) is **deferred to R4** for deletion — for R1, mark it `#[ignore]` with a comment pointing to the R4 cleanup, so the test suite stays green.

5. **Add legacy-dir pruning to `src/services/session/cleanup.rs`.**
    - Add a new function `prune_legacy_pid_dirs(ctx)`.
    - Scan `<XDG_RUNTIME_DIR>/codex-session/sessions/pid-*/` and `<XDG_STATE_HOME>/codex-session/sessions/pid-*/` (i.e., the OLD per-pid layout — not the new accounts/ tree).
    - For each match with `mtime > 24h`: `fs::remove_dir_all(...)`. Log-and-swallow on errors.
    - Call this from the existing cleanup pass (it already runs periodically; just hook in).
    - Add a one-shot first-run flag (`<state-root>/state/.legacy-pruned`) so the pass only fires once per machine, not every invocation. Atomic write via existing fs helper.

6. **Extend `src/commands/doctor.rs` and `src/commands/config_status.rs`.**
    - `doctor` adds a new section:
      ```
      account:           default
      group-id:          pts-0
      group-id-source:   tty
      codex_home:        /home/user/.local/state/codex-session/accounts/default/groups/pts-0
      ```
    - `config status` adds the same to its `--format json` output as new top-level keys (`account`, `group_id`, `group_id_source`, `codex_home`).
    - Update snapshot tests in `tests/cmd_doctor*.rs` and `tests/cmd_config_*.rs`.

7. **Verify env-scrubbing in `src/domain/child_invocation.rs::ChildEnv::scrubbed_default()`.**
    - Confirm (or add) that all `CODEX_SESSION_*` env vars are removed from the child env before exec.
    - Add a regression test in `tests/cmd_passthrough_env.rs` (or a new file) that sets `CODEX_SESSION_GROUP=test`, `CODEX_SESSION_FOO=bar`, runs the wrapper with `CODEX_SESSION_CHILD_BIN=tests/fixtures/echo-env.sh`, and asserts neither var reaches the child.

8. **Tests.** Write:
    - Unit tests in `src/services/session/group_id.rs::tests`: each arm of the resolution chain fires correctly; `pid-N` arm emits the stderr warning exactly once; `GroupId::from_str` validation.
    - Integration test `tests/group_id_resolution.rs`: `--group foo` overrides env; env overrides everything below.
    - Integration test `tests/session_dir_persistence.rs`: two consecutive invocations with the same group-id share the same `CODEX_HOME`; `<root>/accounts/default/groups/<gid>/` exists after first invocation; second invocation finds the rollout from the first via `codex exec resume` (use a fake codex that writes a rollout file and reads it back).
    - Integration test `tests/auth_bridge_seed_and_persist.rs`: `auth.json` is imported once on first invocation; second invocation does NOT overwrite it (one-shot semantics).
    - Snapshot test updates for `doctor` and `config status`.
    - Legacy-prune test: create fake `sessions/pid-*/` dirs with old mtime, run cleanup, verify they're gone; verify the `.legacy-pruned` marker is created and subsequent runs no-op.

## Files touched (representative)

- `src/services/session/group_id.rs` (NEW)
- `src/services/session/terminal_id.rs` (DELETE — or keep as a `pub use super::group_id::*;` alias for one minor version if external callers exist; the project is internal-only, so deletion is fine)
- `src/services/session/dir.rs` (path layout)
- `src/services/session/cleanup.rs` (legacy-prune pass)
- `src/services/session/mod.rs` (re-exports)
- `src/services/auth.rs` (one-shot importer)
- `src/commands/pass_through.rs` (no more watcher wiring)
- `src/cli/mod.rs` (GlobalArgs gains `--group`)
- `src/commands/doctor.rs`, `src/commands/config_status.rs` (report new layout)
- `src/domain/child_invocation.rs` (env-scrubbing verification + possible fix)
- `tests/group_id_resolution.rs` (NEW)
- `tests/session_dir_persistence.rs` (NEW)
- `tests/auth_bridge_seed_and_persist.rs` (refit; `#[ignore]` the watcher test until R4)
- `tests/cmd_passthrough_env.rs` (env-scrub regression)
- `tests/cmd_doctor*.rs`, `tests/cmd_config_*.rs` (snapshot updates)

**Net LOC estimate:** ~450–550 (incl. ~80 LOC of test fixtures). **New tests:** ~8–10.

## Done criteria

```sh
just precommit-all     # must exit 0
```

Plus manual smoke (run after install):

```sh
codex-session exec --json "echo hello" > /tmp/r1.jsonl
THREAD=$(jq -r 'select(.type=="thread.started") | .thread_id' /tmp/r1.jsonl | head -1)
codex-session exec resume "$THREAD" --json "echo round 2"
# MUST succeed (currently fails on main with "no rollout found")
```

Also:

```sh
# Verify warning fires only on pid-N fallback
CODEX_SESSION_GROUP=test codex-session exec --json "hi" 2>&1 | grep "falling back"   # MUST be empty
codex-session exec --json "hi" </dev/null 2>&1 | grep "falling back"                  # MUST emit warning (no TTY, no env)
```

## Out of scope for Round 1

- Top-level `account` subcommand (Round 2).
- Multi-account selection / `[account]` config section (Round 2).
- Quota reader (Round 3).
- Reactive 429 failover (Round 4).
- `--account <name>` global flag (Round 2).
- Deleting `auth/watcher.rs` and `auth/signal.rs` (Round 4 — they stay compiled-but-unused for now).

## Constraints / project conventions (must follow)

- Use `just` recipes for verification (`just lint`, `just test-unit`, `just test-integration`, `just precommit-all`), never raw `cargo`.
- One file per subcommand on both sides (cli + commands) per the four-edit rule (cli-spec §02). R1 doesn't add new subcommands but keeps the existing pattern.
- No `println!` / `eprintln!` outside `src/ui/`, `src/error.rs`, `src/logging.rs`, `tests/` (print-ownership lint enforced in `justfile`).
- Structured logging via `tracing` with `op=` keys. New ops in R1: `group_id.fallback` (warn-level when pid-N arm fires).
- Errors use the existing `AppError` / `ConfigError` / `AuthError` enums with stable `.kind()` keys.
- `pub(crate)` visibility for all new types unless they cross a module boundary.
