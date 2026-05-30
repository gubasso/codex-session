# Multi-Account Architecture Reference

How upstream and community projects solve multi-account OAuth management
for the Codex CLI, and how `codex-session`'s design draws from them.

- **Last verified:** 2026-05-26

---

## 1. Problem statement

Multi-account auth for Codex CLI is hard because of three compounding
constraints:

1. **Single-use refresh tokens (rotation):** each refresh invalidates the old token permanently. Two processes refreshing the same token race, and the loser's token is dead.
2. **Server-side session invalidation:** `codex login` revokes any previously-stored token (PR #21747), and `codex logout` revokes server-side (PR #17825). A shared `~/.codex/auth.json` means each login cycle kills the previous account.
3. **Hardcoded client ID:** all codex sessions use `app_EMoamEEZ73f0CkXaXp7hrann`. There is no per-user or per-profile client registration.

The upshot: you cannot have multiple valid sessions sharing a single
`CODEX_HOME`.

## 2. Upstream feature request

[Issue #4432 — First-class multi-account auth via `--auth-profile`](https://github.com/openai/codex/issues/4432)

Proposed API:

- `codex login --auth-profile work` → stores under
  `~/.codex/profiles/work/`
- `CODEX_PROFILE=work` env var for default profile selection
- Full backward compatibility — no `--auth-profile` falls back to
  `~/.codex/`
- Profile name validation to prevent directory traversal

Status: working implementation complete, PR pending upstream team
acknowledgment. Not yet merged as of 2026-05-26.

## 3. Community projects

### 3.1 codex-lb

**Repository:** [github.com/Soju06/codex-lb](https://github.com/Soju06/codex-lb)

**Architecture:** Python FastAPI proxy service with database backend.

| Aspect          | Detail                                                                               |
| --------------- | ------------------------------------------------------------------------------------ |
| Type            | Centralized proxy service                                                            |
| Language        | Python 3.13+ (FastAPI)                                                               |
| Token storage   | SQLite or PostgreSQL (one row per account)                                           |
| Token refresh   | Built-in OAuth service with configurable interval                                    |
| Account routing | Budget-safe filtering → health-tier cascade → sticky sessions via `prompt_cache_key` |
| Deployment      | Docker, `uvx`, or Kubernetes/Helm                                                    |

**Key design choices:**

- Accounts are database records, not filesystem directories — enables
  centralized management, but requires a running service.
- Health tiers: healthy → probing → draining. Rate-limited accounts are
  demoted, not removed.
- Sticky sessions: conversations pin to the same account to preserve
  prompt cache continuity.

### 3.2 CAAM (Coding Agent Account Manager)

**Repository:** [github.com/Dicklesworthstone/coding\_agent\_account\_manager](https://github.com/Dicklesworthstone/coding_agent_account_manager)

**Architecture:** Go CLI, file-based auth swapping.

| Aspect          | Detail                                              |
| --------------- | --------------------------------------------------- |
| Type            | Local file swapper                                  |
| Language        | Go                                                  |
| Token storage   | `~/.local/share/caam/vault/<tool>/<email>/`         |
| Token refresh   | Manual for Claude; automatic for Codex/Gemini       |
| Account routing | Manual switch, round-robin, or smart (health-score) |
| Deployment      | `brew install` or shell script                      |

**Three isolation modes:**

1. **Vault profiles (default):** swap files in place (~50ms activation). One active account per tool at a time.
2. **Isolated profiles:** full directory isolation with pseudo-HOME symlinks. Parallel sessions possible.
3. **Shallow profiles:** minimal isolation (auth files only); for orchestrators.

### 3.3 codex-multi-auth

**Repository:** [github.com/ndycode/codex-multi-auth](https://github.com/ndycode/codex-multi-auth)

**Architecture:** Go CLI wrapper alongside the official codex binary.

| Aspect          | Detail                                             |
| --------------- | -------------------------------------------------- |
| Type            | Local CLI wrapper                                  |
| Language        | Go                                                 |
| Token storage   | `~/.codex/multi-auth/`                             |
| Token refresh   | Proactive with staggered intervals                 |
| Account routing | Health-aware rotation with bounded request budgets |
| Deployment      | CLI binary                                         |

**Key design choices:**

- Proactive token refresh with staggered intervals reduces background
  bursts.
- `refresh_token_reused` detection triggers automatic re-login.
- Per-project account isolation under `~/.codex/multi-auth/projects/`.
- Cross-account 5xx burst detection triggers cooldown instead of
  aggressive rotation.

## 4. Comparative table

| Feature       | codex-lb                    | CAAM              | codex-multi-auth | **codex-session**                              |
| ------------- | --------------------------- | ----------------- | ---------------- | ---------------------------------------------- |
| Type          | Proxy service               | File swapper      | CLI wrapper      | **CLI wrapper**                                |
| Isolation     | Database per-account        | Vault dirs        | Config dirs      | **`CODEX_HOME` per auth op**                   |
| Token refresh | Built-in service            | Manual/auto       | Proactive        | **On-demand (401 retry)**                      |
| Routing       | Budget-safe + health-tier   | Smart/round-robin | Health-aware     | **Quota-aware auto**                           |
| Deployment    | Docker/K8s                  | `brew`            | Binary           | **Cargo**                                      |
| Complexity    | High (daemon + DB)          | Medium            | Medium           | **Low**                                        |
| Thread resume | Not built-in (proxy-scoped) | Not documented    | Not documented   | **Cross-account index (`thread-index.jsonl`)** |

## 5. What codex-session adopted and why

**From codex-lb:** token refresh as a recovery mechanism. Not
proxy-based (too heavy for a CLI wrapper), but the same principle of not
giving up on first 401. Implemented as an on-demand retry in the quota
module.

**From CAAM:** full directory isolation per account. Our `CODEX_HOME`
per auth operation maps directly to their "isolated profiles" concept.
During `codex login`, a fresh temp dir under `state_dir/auth-ops/` is
used as `CODEX_HOME`; during `codex exec`, a permanent per-account
session dir is used.

**From codex-multi-auth:** stale-token detection and transparent
re-auth. When the WHAM API returns 401, we attempt an OAuth token
refresh using the stored `refresh_token` before surfacing the error.

**Three-layer auth defense (added 2026-05):**

1. **Layer 1 — JWT pre-check (selector.rs):** skip accounts with tokens
   expiring within 60 seconds, before attempting quota fetch.
2. **Layer 2 — Quota-level 401 retry (quota.rs):** on HTTP 401 during
   quota fetch, attempt token refresh and retry once.
3. **Layer 3 — Exec-level detection (retry.rs + failover.rs):** scan
   child stderr/stdout for auth-failure patterns; attempt refresh;
   rotate on failure.

See [`docs/openai-api-error-reference.md`](./openai-api-error-reference.md)
for pattern lists and error semantics.

**From upstream #4432:** the `CODEX_HOME`-as-profile pattern. We use
ephemeral temp dirs during auth operations and permanent per-account
session dirs during exec — achieving the same isolation without waiting
for upstream to ship `--auth-profile`.

**What we chose NOT to adopt:**

- Daemon/proxy model (codex-lb): too heavy for a CLI wrapper.
- Database backend: overkill for 3-5 accounts.
- Round-robin rotation: we already have quota-aware `--account auto`.
- Proactive background refresh: adds complexity; on-demand 401-retry is
  simpler and sufficient.

## 6. Thread resume across accounts

### Problem

Upstream codex scopes all session storage to `$CODEX_HOME/sessions/`
(see F6, F14 in `docs/upstream-codex.md`). When a multi-account wrapper
uses different `CODEX_HOME` directories per account, `resume --last`
resolves within a single account's session directory — it cannot find a
session that ran under a different account.

### Solution

`codex-session` maintains an append-only JSONL thread index at
`<state_dir>/thread-index.jsonl`, outside any per-account `CODEX_HOME`.
Each entry records:

| Field        | Type   | Description                                   |
| ------------ | ------ | --------------------------------------------- |
| `thread-id`  | String | Session UUID from `thread.started` JSON event |
| `account`    | String | Account ID that owned the session             |
| `group-id`   | String | Terminal/group ID at session creation time    |
| `cwd`        | String | Working directory at session creation time    |
| `created-at` | String | RFC 3339 UTC timestamp                        |

Field names use kebab-case in the JSONL file (serde `rename_all`).

### Capture

During `codex exec --json` runs, the wrapper tee-captures stdout and
scans for the `{"type":"thread.started","thread_id":"..."}` JSON event.
When found, a `ThreadEntry` is appended to the index. Non-`--json` runs
do not emit structured events and are not indexed. This means interactive
TUI sessions are invisible to the thread index; `--last` resolution only
considers `exec --json` runs.

### Resume routing

When the wrapper detects a resume intent (`exec resume <ID>`,
`exec resume --last`, `resume --last`, `resume --all`), it:

1. Looks up the thread ID in the index (by exact ID for `ById`, by
   most-recent-in-group for `--last`, by most-recent-any for
   `--last --all-groups` or `resume --all`).
2. If found, resolves the account **and original group-id** from the
   index entry (source: `ThreadIndex`), sets `CODEX_HOME` to the
   indexed group's session directory, and rewrites `--last`/`--all` to
   the concrete thread ID before forwarding to codex.
3. If not found, falls back to normal account resolution and forwards
   the original argv (stripping wrapper-only `--all-groups` flag).

### Wrapper-specific flags

- `--all-groups`: wrapper-only flag on `exec resume --last --all-groups`.
  Expands the `--last` search to all terminal groups, not just the
  current one. Stripped before forwarding to codex.
- `resume --all`: upstream flag meaning "show sessions from any
  directory". The wrapper intercepts this as `Last { all_groups: true }`
  for index lookup, then rewrites to a concrete thread ID for codex.
  Note: this bypasses upstream's interactive session picker — the wrapper
  auto-selects the most recent session instead.

### Community comparison

None of the surveyed community projects (`codex-lb`, `CAAM`,
`codex-multi-auth`) document cross-account resume support. `codex-lb`
inherits whatever resume the upstream CLI provides, scoped to its
proxy-managed `CODEX_HOME`.
