# 00 — Overview

## North star

Replace `codex-session`'s ephemeral per-call `CODEX_HOME` with a persistent two-axis `accounts/<account>/groups/<group-id>/` layout. Same change enables (a) native `codex exec resume` across invocations, fixing the bug that breaks `/prex` and `/review-loop` rounds 2+, and (b) the planned ChatGPT-plan multi-account auto-load-balancer (account selection, quota awareness, reactive 429 failover) without rework.

## Why now

The current wrapper's `terminal_id::current()` (`src/services/session/terminal_id.rs:7-9`) silently falls back to `pid-{getpid()}` whenever no controlling TTY is attached. Every headless invocation (Claude-Code-driven `/prex`, `/review-loop`, agent subprocesses) thus gets a *different* `CODEX_HOME`. Verified empirically: `~/.local/state/codex-session/sessions/pid-21363/`, `pid-58373/`, `pid-129046/`, … one dir per call.

Upstream codex stores rollouts under `CODEX_HOME/sessions/YYYY/MM/DD/rollout-*.jsonl` and resolves thread-ids via `find_thread_path_by_id_str(codex_home, id)` (state DB backfilled from that tree). Resume cannot find a prior rollout across invocations because `CODEX_HOME` is different each time. The session-resume contract is broken for the entire class of agent-driven workflows.

## What this plan delivers

1. **Bug fix:** persistent `CODEX_HOME` keyed on a resolved `group-id` (5-step chain ending in a *warned* `pid-N` fallback, not a silent one).
2. **Multi-account foundation:** top-level `codex-session account add/list/current/use/remove` subcommand and `--account <name>` global flag.
3. **Quota awareness:** inline Rust client for `https://chatgpt.com/backend-api/wham/usage`, defensive parser, cache, caam-borrowed scoring selector. `--account auto` becomes proactive.
4. **Reactive failover:** caam-borrowed regex 429 detector + per-account cooldown + retry-with-rotation.
5. **AuthBridge cleanup:** demote to one-shot importer; delete watcher/signal/persist-on-drop.

## What this plan does NOT do (non-goals)

- Mid-session account rotation (ndycode's loopback proxy pattern). Between-invocation rotation is sufficient for the `codex exec` workload.
- Fork upstream codex / build a TUI variant (Loongphy/`codext` pattern).
- Host a rotation-proxy daemon (Soju06/`codex-lb` pattern).
- Encrypted `auth.json` vault (caam Vault, prakersh) — per-account `CODEX_HOME` already isolates credentials.
- A `self` namespace — see [04-decisions.md#d8](04-decisions.md) (D8) for rationale.
- A picker UI for `--account auto` — `auto` is non-interactive; the selector chooses silently.

## Success criteria

- `codex-session exec --json "first" > /tmp/r.jsonl && codex-session exec resume "$(jq -r '...thread_id' /tmp/r.jsonl)" --json "second"` **succeeds** in any context (interactive shell, Claude-Code child process, cron job).
- `codex-session account add work && codex-session --account work exec "..."` routes to `~/.local/state/codex-session/accounts/work/groups/<group-id>/` for storage.
- `codex-session --account auto exec "..."` proactively picks the highest-scoring eligible account; falls over to the next account on HTTP 429.
- `just precommit-all` clean. All 38 existing integration tests pass after R1; new tests land per round.

## Glossary

| Term | Meaning |
|---|---|
| **account** | A named ChatGPT-plan credential set. Persists at `<state-root>/accounts/<name>/`. Owns its own `auth.json` and rollouts. |
| **group-id** | Stable identifier scoping a logical session (typically per-terminal). Determines which `groups/<group-id>/` dir under an account is used as `CODEX_HOME`. Resolved via env → flag → TTY → PPID+starttime → `pid-N` (warned). |
| **CODEX_HOME** | The dir passed to upstream codex as `$CODEX_HOME`. Resolves to `<state-root>/accounts/<account>/groups/<group-id>/`. |
| **state-root** | `$XDG_STATE_HOME/codex-session/` (typically `~/.local/state/codex-session/`). |
| **selector** | The `services/account/selector.rs::pick()` function that scores accounts by quota + health + recency + plan, returns the best eligible. |
| **cooldown** | Per-account `accounts/<name>/cooldown.json` record with `reset_at_unix`. Account is disqualified from selection until `now > reset_at`. |
| **AuthBridge** | The pre-refactor `services/auth.rs` machinery that watched + persisted auth.json between native `~/.codex` and the ephemeral CODEX_HOME. **Demoted in R1 to one-shot importer; sub-modules deleted in R4.** |
| **prex round** | One full `/prex` invocation (plan → review → implement → review → optional review-loop). One round per phase of this plan. |

## Where to read next

- [01-architecture.md](01-architecture.md) — current vs target layout, group-id resolution, AuthBridge demotion.
- [04-decisions.md](04-decisions.md) — every load-bearing design choice with rationale.
- [05-cli-design.md](05-cli-design.md) — subcommand tree, flags, conformance to the project CLI design SoT.
- [10-phase-foundation.md](10-phase-foundation.md) — Round 1 prex input (this is where execution starts).
- [99-execution-plan.md](99-execution-plan.md) — phase grouping, difficulty, sequencing, verification.
