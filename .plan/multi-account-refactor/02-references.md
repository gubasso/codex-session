# 02 — References (upstream codex SoT)

Verified facts about upstream codex CLI behavior that this refactor depends on. All claims here are cross-referenced against `docs/upstream-codex.md` (the project's existing upstream-behavior reference) and verified against upstream source / issues / docs.

**Last verified:** 2026-05-22 against codex-cli 0.132.0.

---

## F1 — `CODEX_HOME` semantics

`CODEX_HOME` defaults to `~/.codex` and **redirects all codex state** when set: `config.toml`, `auth.json`, `history.jsonl`, `memories/`, `plugins/cache/...`, `session_index.jsonl`, `state_5.sqlite`, `logs_2.sqlite`, MCP server config + caches, `sessions/`, `archived_sessions/`. There are **no hard-coded fallbacks** to `~/.codex/*` in write paths.

**Source:** `docs/upstream-codex.md` F6; OpenAI docs ["local-config"](https://developers.openai.com/codex/local-config/), PR [openai/codex#20667](https://github.com/openai/codex/pull/20667), issues [#18065](https://github.com/openai/codex/issues/18065) and [#4407](https://github.com/openai/codex/issues/4407).

**Implication for this refactor:** any persistent `CODEX_HOME` path we choose isolates *all* codex state. Our two-axis `accounts/<account>/groups/<group_id>/` layout works because of this.

---

## F2 — Storage layout under `CODEX_HOME`

| File / dir | Scope | Notes |
|---|---|---|
| `config.toml` | global per-`CODEX_HOME` | Composed from profile layers by our wrapper |
| `auth.json` | global per-`CODEX_HOME` | OAuth tokens or API key; one credential at a time |
| `history.jsonl` | global per-`CODEX_HOME` | Shell-prompt history accumulator |
| `memories/` | global per-`CODEX_HOME` | Persistent user / project memories |
| `plugins/cache/...` | global per-`CODEX_HOME` | Plugin runtime cache |
| `session_index.jsonl` | global per-`CODEX_HOME` | Discovery index synchronized with state DB |
| `state_5.sqlite` | global per-`CODEX_HOME` | Thread index; backfilled from `sessions/` at startup |
| `logs_2.sqlite` | global per-`CODEX_HOME` | Activity logs |
| MCP config | in `config.toml` | MCP runtime/auth/cache under same `CODEX_HOME` umbrella |
| `sessions/YYYY/MM/DD/rollout-*.jsonl` | per-thread | Newline-JSON event stream; immutable once written |
| `archived_sessions/...` | per-thread | Archived rollouts |

**Source:** OpenAI docs ["Advanced Configuration"](https://developers.openai.com/codex/config-advanced); upstream source `raw.githubusercontent.com/openai/codex/main/codex-rs/core/src/rollout/list.rs`, `raw.githubusercontent.com/openai/codex/main/codex-rs/message-history/src/lib.rs`, `raw.githubusercontent.com/openai/codex/main/codex-rs/core/src/thread_manager.rs`; issues [#20864](https://github.com/openai/codex/issues/20864) (backfill perf), [#21196](https://github.com/openai/codex/issues/21196) (rollout discovery).

---

## F3 — Resume contract

`codex exec resume <thread-id>` discovers the rollout file via `find_thread_path_by_id_str(codex_home, id)` — **lookup is `CODEX_HOME`-rooted**, backfilled from `sessions/` into `state_5.sqlite` at startup.

Stable CLI flags expose only `--last`, `--all`, and `--session-id`. There is no stable `--rollout` / `--path` flag.

Upstream protocol `ThreadResumeParams` (unstable, internal) supports `history` and `path` fields with precedence `history > path > thread_id`. Not surfaced as CLI flags.

**Source:** OpenAI docs ["Codex CLI reference"](https://developers.openai.com/codex/cli/reference); ["Resume OpenAI Codex CLI Sessions"](https://inventivehq.com/knowledge-base/openai/how-to-resume-sessions); ["Rollout Persistence and Replay"](https://deepwiki.com/openai/codex/3.5.2-rollout-persistence-and-replay); upstream `raw.githubusercontent.com/openai/codex/main/codex-rs/app-server-protocol/src/protocol/v2.rs`; issues [#21196](https://github.com/openai/codex/issues/21196), [#11634](https://github.com/openai/codex/issues/11634), [#19822](https://github.com/openai/codex/issues/19822), [#22452](https://github.com/openai/codex/issues/22452).

**Implication for this refactor:** persistent `CODEX_HOME` per `(account, group)` makes resume work natively — no need to use the unstable protocol path.

---

## F4 — Multi-account: no native support

Codex CLI has **no native multi-account support**. Login surface is single-credential: `codex login`, `codex login --device-auth`, `codex login --with-api-key`, `codex login status`, `codex logout`. `--profile` / `-p` selects *config* profiles, not auth identities. `~/.codex/auth.json` holds exactly one credential at a time.

Two upstream feature requests track this need (no OpenAI engagement yet):

- [openai/codex#4432](https://github.com/openai/codex/issues/4432) — "First-class multi-account auth via `--auth-profile`". Proposes `--auth-profile <name>`, `CODEX_PROFILE` env var, state under `~/.codex/profiles/<name>/`. Open since 2025-09-29.
- [openai/codex#9648](https://github.com/openai/codex/issues/9648) — "Multi-account ChatGPT OAuth rotation and management". Proposes cooldown skip, 401/403 refresh-and-retry, `Retry-After` honoring, `codex login accounts`, `codex logout --account/--all-accounts`, TUI health summary. Open since 2026-01-22. Has a draft branch `oauth-marathon`.

**`CODEX_HOME` env var is the officially-supported lever for per-account isolation.**

**Source:** [Codex CLI reference](https://developers.openai.com/codex/cli/reference); [Codex Auth docs](https://developers.openai.com/codex/auth).

---

## F5 — Quota endpoint (`wham/usage`)

Codex CLI itself polls `GET https://chatgpt.com/backend-api/wham/usage` roughly **every 60 seconds** while the TUI is running ([openai/codex#10869](https://github.com/openai/codex/issues/10869)). The endpoint is the de-facto third-party lever for reading quota.

### Headers (required)

```
Authorization: Bearer <access_token>
ChatGPT-Account-Id: <account_id>
Accept: application/json
Origin: https://chatgpt.com
Referer: https://chatgpt.com/
User-Agent: Mozilla/5.0
```

Both `access_token` and `account_id` live in `~/.codex/auth.json`.

### Response shape (varies by version)

A `rate_limit` (or `rate_limits`) object containing `five_hour` (or `primary_window`) and `weekly` (or `secondary_window`) sub-objects. Each carries fields like `percent_left`, `used_percent`, `reset_at` / `reset_time_ms`, `limit_window_seconds`. Per-model and code-review windows also exposed.

### Defensive parsing

Knightli's research warns: **"read raw fields, not derived labels"** — response shape has already changed between versions. Treat label fields (`label`, `status`) as opaque; parse only the raw numeric fields you depend on; tolerate unknown extra keys.

### Caveats

- Undocumented internal endpoint. Schema drift is expected.
- The `wham/usage` poller fires even in API-key mode ([#10869](https://github.com/openai/codex/issues/10869)).
- Alternative endpoint observed by `codex-cli-usage` (PyPI): `/backend-api/codex/usage`.

**Source:** [Knightli — Codex usage quota check](https://www.knightli.com/en/2026/04/12/codex-usage-quota-check/), [Knightli — Codex usage limits](https://www.knightli.com/en/2026/04/15/codex-usage-limits-five-hour-weekly-credits/); [Loongphy/codex-auth](https://github.com/Loongphy/codex-auth) source.

**Implication for this refactor:** R3's `services/account/quota.rs` parses defensively (raw fields only); cached at 30 s TTL; rejects responses missing both `rate_limit` and `rate_limits` with `QuotaParseFailed`.

---

## F6 — Token-refresh write-back race

Codex periodically refreshes OAuth tokens and writes back to `auth.json`. No documented `flock` on `auth.json`. If two codex processes share the same `auth.json` and one refreshes while another holds the in-memory copy, the second's eventual write can overwrite the refreshed token (and vice-versa).

**Implication for this refactor:** sidestepped entirely by per-account `CODEX_HOME` (each account has its own `auth.json`; no inter-process contention). Documented in ADR [D3](04-decisions.md). The pre-refactor `AuthBridge`'s `last_refresh` timestamp-guard logic is no longer needed and is deleted in R4.

**Source:** `.plan/codex-session-multi-account-research.md` §5; observed in `Sls0n/codex-account-switcher`, `denysdovhan/codex-account`, `bashar94/codex-cli-account-switcher` (all unfixed).

---

## F7 — Trust schema & write-back model

Codex uses `toml_edit` to modify `config.toml`, preserving user formatting and comments where possible, paired with atomic `tempfile + rename` writes. Trust-persistence failures are log-and-swallow — codex does not abort the session on write error.

**Source:** `docs/upstream-codex.md` F7; upstream PR [#17595](https://github.com/openai/codex/pull/17595); `codex-rs/core/src/config.rs`.

**Implication for this refactor:** unchanged. Trust sync in `src/services/trust_sync.rs` continues to work post-refactor (it operates on whatever `CODEX_HOME` we hand it).

---

## F8 — Trust-prompt gating

Codex skips the per-launch trust prompt when any of the following hold: global `approval_policy` is set; global `sandbox_mode` is set; yolo mode / `--dangerously-bypass-approvals-and-sandbox` is active; or the project already has an explicit trust entry.

**Source:** `docs/upstream-codex.md` F5; issues [#14547](https://github.com/openai/codex/issues/14547), [#9695](https://github.com/openai/codex/issues/9695).

**Implication for this refactor:** trust sync in our wrapper naturally no-ops in these modes (existing `TrustSyncOutcome::Unchanged`). Untouched.

---

## F9 — Stale path canonicalization

Codex canonicalizes all configured project paths at startup. If any path is missing/invalid, codex returns an error during config load.

**Source:** `docs/upstream-codex.md` F8.

**Implication for this refactor:** when account-aware paths are constructed, ensure the parent dirs exist before codex starts. Existing `secure_dir()` handles this.

---

## Project-internal references

- `docs/upstream-codex.md` — the live SoT for upstream codex facts. Keep updated as upstream evolves.
- `.plan/codex-session-multi-account-research.md` — original research notes (now consolidated into this dir; the file itself is rewritten as a pointer in `.plan/codex-session-multi-account-research.md`).
- `CLAUDE.md` — project quality gates (use `just`, not raw `cargo`).
- `src/services/auth.rs`, `src/services/session/{dir,terminal_id,cleanup}.rs`, `src/commands/pass_through.rs` — current implementation.

---

## External references (master list)

### OpenAI codex docs

- [Authentication — Codex](https://developers.openai.com/codex/auth)
- [Command-line reference](https://developers.openai.com/codex/cli/reference)
- [Advanced Configuration](https://developers.openai.com/codex/config-advanced)
- [Local config (~/.codex)](https://developers.openai.com/codex/local-config/)
- [Pricing](https://developers.openai.com/codex/pricing)
- [Using Codex with ChatGPT plan](https://help.openai.com/en/articles/11369540-using-codex-with-your-chatgpt-plan)
- [Codex rate card](https://help.openai.com/en/articles/20001106-codex-rate-card)

### Upstream codex source

- `codex-rs/core/src/rollout/list.rs` — `find_thread_path_by_id_str`
- `codex-rs/app-server-protocol/src/protocol/v2.rs` — `ThreadResumeParams`
- `codex-rs/core/src/thread_manager.rs`
- `codex-rs/message-history/src/lib.rs`
- `codex-rs/core/src/config.rs` — trust write-back

### Upstream codex issues

- [#4407 — change hardcoded `$HOME/.codex` path](https://github.com/openai/codex/issues/4407)
- [#4432 — first-class multi-account auth](https://github.com/openai/codex/issues/4432)
- [#9648 — multi-account OAuth rotation](https://github.com/openai/codex/issues/9648)
- [#9695 — YOLO suppresses trust prompt but blocks .codex/skills](https://github.com/openai/codex/issues/9695)
- [#10869 — constant requests to wham/usage](https://github.com/openai/codex/issues/10869)
- [#11634 — state db missing rollout path](https://github.com/openai/codex/issues/11634)
- [#14547 — trust prompt appears every launch despite yolo](https://github.com/openai/codex/issues/14547)
- [#15396 — ...](https://github.com/openai/codex/issues/15396)
- [#18065 — misleading `~/.codex/config.toml` references ignore `$CODEX_HOME`](https://github.com/openai/codex/issues/18065)
- [#19822 — codex local history index stale](https://github.com/openai/codex/issues/19822)
- [#20084 — ...](https://github.com/openai/codex/issues/20084)
- [#20213 — ...](https://github.com/openai/codex/issues/20213)
- [#20399 — ...](https://github.com/openai/codex/issues/20399)
- [#20667 (PR) — load configured environments from CODEX_HOME](https://github.com/openai/codex/pull/20667)
- [#20864 — codex desktop laggy scanning sessions](https://github.com/openai/codex/issues/20864)
- [#21138 — ...](https://github.com/openai/codex/issues/21138)
- [#21196 — data loss: resumed-thread errors due missing rollout JSONL](https://github.com/openai/codex/issues/21196)
- [#22452 — codex desktop state_5.sqlite drift on Windows](https://github.com/openai/codex/issues/22452)
- [Discussion #2251 — codex usage limits](https://github.com/openai/codex/discussions/2251)
- [PR #10745 — resumable backfill](https://github.com/openai/codex/pull/10745)
- [PR #17595 — atomic config.toml writes](https://github.com/openai/codex/pull/17595)

### Quota / usage research

- [Knightli — Codex usage quota check](https://www.knightli.com/en/2026/04/12/codex-usage-quota-check/)
- [Knightli — Codex usage limits explained](https://www.knightli.com/en/2026/04/15/codex-usage-limits-five-hour-weekly-credits/)
- [OpenAI Community — Understanding new Codex limits (Apr 9)](https://community.openai.com/t/understanding-the-new-codex-limit-system-after-the-april-9-update/1378768)
- [codex-cli-usage (PyPI)](https://pypi.org/project/codex-cli-usage/)

### Cross-surface sync

- [Daniel Vaughan — Cross-Surface Session Sync](https://codex.danielvaughan.com/2026/04/08/cross-surface-session-sync/)
