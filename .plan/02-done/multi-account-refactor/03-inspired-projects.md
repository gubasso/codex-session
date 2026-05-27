# 03 — Inspired Projects

Per-project comparison of every reviewed solution in the multi-account / CODEX_HOME-isolation space. What we borrowed, what we left, and why.

GitHub stats verified via `gh api repos/<owner>/<repo>` on 2026-05-17. Consolidates the original research at `.plan/codex-session-multi-account-research.md` §6, plus the second-round /ask synthesis that examined each project's actual source code (not just READMEs).

---

## Tier A — Closest analogues

### `Dicklesworthstone/coding_agent_account_manager` (caam)

- **Repo:** <https://github.com/Dicklesworthstone/coding_agent_account_manager>
- **Stars / last push:** 124★ / 2026-05-06
- **Lang / license:** Go / MIT-ish (with OpenAI/Anthropic Rider)
- **What it does:** Sub-100 ms file-based auth swap across Claude Code / Codex / Gemini. `caam activate <tool> --auto` picks best config_recipe via multi-factor scoring. `caam run` is a transparent wrapper that auto-detects rate limits and fails over to the next config_recipe. Two modes:
  - **Vault Profiles** — swaps `auth.json` in place (the dominant abstraction).
  - **Isolated Profiles** (`caam exec`) — per-config_recipe `$HOME` + `$CODEX_HOME` (effectively Option C).
- **Fit:** 4/5 — closest transparent-wrapper UX; multi-tool; sub-100ms swap. Rotation is reactive (post-429), NOT proactive at 50%.

**What we borrowed:**

- **Regex 429 detector** (verbatim): `(?i)\b(429|rate[- ]limit|too many requests|quota exceeded|slow down)\b`. Source: `raw.githubusercontent.com/Dicklesworthstone/coding_agent_account_manager/refs/heads/main/internal/ratelimit/detector.go`.
- **Scoring formula** (verbatim): cooldown disqualifying; health `±100/±50`; `penalty * 10` subtraction; plan bonus `+30` enterprise / `+20` pro|team; recency penalty for recent use, reward for long-idle; real-time availability `availScore - 50`, additional `-30` if weekly is high. Source: `raw.githubusercontent.com/Dicklesworthstone/coding_agent_account_manager/refs/heads/main/internal/rotation/rotation.go`.
- **Retry-with-rotation harness shape** (`caam run` wraps exec, scans stdout/stderr, retries on 429 with the next eligible config_recipe). Source: `raw.githubusercontent.com/Dicklesworthstone/coding_agent_account_manager/refs/heads/main/internal/wrap/wrap.go`.

**What we left:**

- **Vault Profiles auth-swap mode** — has the token-refresh write-back race we sidestep with per-account `CODEX_HOME` (see [02-references.md](02-references.md) F6 and ADR [D3](04-decisions.md)).
- **Daemon / TUI** — codex-session is CLI-only; no resident process.
- **Multi-tool abstraction** — codex-session is codex-only; no Claude/Gemini support layer.

---

### `ndycode/codex-multi-auth`

- **Repo:** <https://github.com/ndycode/codex-multi-auth>
- **Stars / last push:** 271★ / 2026-05-11
- **Lang / license:** TypeScript / MIT
- **What it does:** Account pool with explicit `switch`, plus default-on **runtime Responses rotation proxy** — a loopback-only local bridge that rotates accounts *between requests* during a forwarded Codex session. Health checks, `forecast --live`, quota cache at `~/.codex/multi-auth/quota-cache.json`, reactive 429 handling, JSON diagnostics. Supports ChatGPT-plan OAuth as primary use case.
- **Architecture:** CLI manager + optional wrapper `codex-multi-auth-codex` + local loopback HTTP proxy (`/v1/responses`).
- **Storage:** `~/.codex/multi-auth/...` plus per-project `~/.codex/multi-auth/projects/<project-key>/...` (not the same shape as our two-axis layout).
- **Fit:** 5/5 for mid-session rotation within a TUI; 0/5 for between-invocation scripted use (needs loopback proxy daemon running and plugin infrastructure).

**What we borrowed:**

- **Diagnostics surface** — surface "why was this account picked" / cooldown observability in `codex-session account list` and `account current --json`.
- **Quota cache TTL** — 30 s TTL pattern.

**What we left:**

- **Renamed binary** (`codex-multi-auth-codex`) — breaks the transparent-passthrough invariant (rejected by ADR [D4](04-decisions.md)).
- **Loopback HTTP proxy** — overkill for between-invocation rotation; revisit only if long-lived TUI use emerges (ADR [D4](04-decisions.md)).
- **Plugin infrastructure** — orthogonal to our CLI-wrapper shape.

---

### `Loongphy/codex-auth` + `codext`

- **Repos:** <https://github.com/Loongphy/codex-auth> + <https://github.com/Loongphy/codext>
- **Stars / last push:** codex-auth 1,757★ / 2026-05-16 (most-starred in this space); codext 80★ / 2026-05-17
- **Lang / license:** Zig (codex-auth) / MIT; Rust (codext, fork of openai/codex) / Apache-2.0
- **What they do:**
  - `codex-auth` manages `auth.json` snapshots; refreshes per-account usage by calling `https://chatgpt.com/backend-api/wham/usage` (with `--skip-api` falling back to scanning `~/.codex/sessions/*/rollout-*.jsonl`). Interactive `switch` only — no auto-rotation, no daemon.
  - `codext` is a forked Codex TUI that **watches `auth.json` for external changes** via filesystem notifications, debounces, hot-reloads at safe boundaries, and dispatches a recovery prompt after rate-limit hits.
- **Fit:** `codex-auth` alone 3/5 (manual); `codext` alone 2/5 (no rotation logic); composed 4/5.

**What we borrowed:**

- **`wham/usage` call pattern + header set** — basis for our inline Rust implementation in R3. Source: `Loongphy/codex-auth` source + knightli's writeup ([02-references.md](02-references.md) F5).

**What we left:**

- **`codex-auth` as a shell-out dep** — rejected in ADR [D5](04-decisions.md) in favor of an inline ~200 LOC Rust client.
- **`codext` fork** — maintenance burden; hot-reload is redundant once each account owns its own persistent `CODEX_HOME` (the AuthBridge demotion in ADR [D3](04-decisions.md) achieves the same effect).

**Release notes:** [Codex Auth v0.2 — API Usage and Automatic Account Switching](https://loongphy.com/en/blog/codex-auth-v02-release/)

---

### `Soju06/codex-lb`

- **Repo:** <https://github.com/Soju06/codex-lb>
- **Stars / last push:** 1,496★ / 2026-05-17
- **Lang / license:** Python (FastAPI) / MIT
- **What it does:** Codex/ChatGPT multiple account load balancer & proxy with usage tracking, dashboard (port 2455), and OpenCode-compatible endpoints (port 1455). Per-account tokens/cost/28-day trends. Client points `base_url` at the local proxy.
- **Backend auth model (verified):** OAuth-pool (not API-key-only). Accounts stored with encrypted `access_token`, `refresh_token`, `id_token`, refreshed via OAuth flows. Source: `raw.githubusercontent.com/Soju06/codex-lb/refs/heads/main/app/modules/oauth/service.py`, `raw.githubusercontent.com/Soju06/codex-lb/refs/heads/main/app/modules/accounts/auth_manager.py`. The client-facing proxy is protected by its own API keys, but the backend is genuine OAuth pool.
- **Architecture:** Standalone Python service (Docker or `uvx`), not a wrapper.
- **Fit:** 5/5 if you want rotation-as-a-service; 0/5 for transparent CLI wrapper shape.

**What we borrowed:**

- **Dashboard idea** — listed as a post-merge follow-up in [99-execution-plan.md](99-execution-plan.md).

**What we left:**

- **Wholesale adoption** — wrong shape (rejected by ADR [D4](04-decisions.md)). Solves a different problem than codex-session's transparent fork/exec wrapper.

---

### `prakersh/codexmultiauth` (cma)

- **Repo:** <https://github.com/prakersh/codexmultiauth>
- **Stars / last push:** 22★ / 2026-04-22
- **Lang / license:** Go / GPL-3.0
- **What it does:** Encrypted multi-account vault; `cma auto` picks best account by remaining quota / reset urgency; includes watcher script for auto-rotation.
- **Fit:** 3.5/5 — promising selection logic; low adoption.

**What we borrowed:** nothing concrete (caam's scoring formula is better-documented and battle-tested).

**What we left:**

- **Encrypted vault** — orthogonal feature; per-account `CODEX_HOME` already isolates credentials at the filesystem level (ADR [D2](04-decisions.md)).
- **GPL-3.0 license** — restrictive for vendoring.

---

## Tier B — Architectural references

### `Spielewoy/multi-codex`

- **Repo:** <https://github.com/Spielewoy/multi-codex>
- **Stars / last push:** 65★ / 2026-05-13
- **Lang / license:** Shell / no clear license
- **What it does:** Run multiple isolated Codex CLI instances at once via `CODEX_HOME`. Each config_recipe gets its own account, config, sessions, skills, agents. No quota balancing.
- **Validates:** the per-`CODEX_HOME` isolation pattern (the foundation of our two-axis layout). Single-axis (account-only); doesn't solve the group-id problem.

### `Ducksss/codex-profiles`

- **Repo:** <https://github.com/Ducksss/codex-profiles>
- **Stars / last push:** 1★ / 2026-05-13
- **Lang / license:** Shell / MIT
- **What it does:** Launches Codex CLI/Desktop with a named `CODEX_HOME`. Each config_recipe fully isolated (auth + config + sessions + logs). Manual selection (`codex-config_recipe login work`, `codex-config_recipe app personal`); no rotation, no quota.
- **Validates:** the *cleanest* expression of the per-`CODEX_HOME` pattern. Architectural template referenced by `.plan/codex-session-multi-account-research.md` §6.

**Announcement:** [codex-profiles — OpenAI Developer Community](https://community.openai.com/t/codex-profiles-switch-codex-accounts-without-copying-auth-json/1380415)

### `wakamex/codex-cli-usage` (PyPI)

- **Repo / pkg:** <https://pypi.org/project/codex-cli-usage/> (v0.1.7, 2026-04-16)
- **Lang:** Python
- **What it does:** Reads quota by calling `/backend-api/codex/usage`; outputs JSON or statusline; has a daemon cache mode. No multi-account, no rotation — but a clean "quota reader" component.
- **Reference value:** demonstrates the alternative endpoint `/backend-api/codex/usage` (we use `wham/usage`).

### `Liam-Deacon/codex-usage`

- **Repo:** <https://github.com/Liam-Deacon/codex-usage>
- **Stars / last push:** 2★ / 2026-02-16
- **Lang / license:** Rust / MIT
- **What it does:** Multi-account quota tracker with "automatic cycling when limits are exhausted"; configurable thresholds close to our design.
- **Reference value:** validates the configurable-threshold idea (we use floor=10% for weekly, gate=50% for five-hour, both config-overridable). Tiny adoption.

---

## Tier C — Manual swap primitives (reference only)

| Project | Stars | Lang | What we learned |
|---|---|---|---|
| [`Sls0n/codex-account-switcher`](https://github.com/Sls0n/codex-account-switcher) | 37 | TS / MIT | Snapshot/swap of `auth.json`. Demonstrates the unfixed token-refresh race ([02-references.md](02-references.md) F6). |
| [`denysdovhan/codex-account`](https://github.com/denysdovhan/codex-account) | 37 | Shell / MIT | Minimal `save`/`switch`/`list`. Same race. |
| [`bjesuiter/codex-switcher`](https://github.com/bjesuiter/codex-switcher) | 6 | TS / MIT | Multi-provider (codex/opencode/pi). |
| [`bashar94/codex-cli-account-switcher`](https://github.com/bashar94/codex-cli-account-switcher) | 81 | Bash / MIT | Backs up entire `~/.codex` dir as zip. Overkill vs CODEX_HOME. |

**All of these are demonstrations of the swap-based pattern that ADR [D3](04-decisions.md) sidesteps by retiring `AuthBridge`.**

---

## Tier D — Disqualified for this use case

- [`Lampese/codex-switcher`](https://github.com/Lampese/codex-switcher) — 312★ Tauri **desktop GUI**, not scriptable into a transparent CLI wrapper.
- [`isxlan0/Codex_AccountSwitch`](https://github.com/isxlan0/Codex_AccountSwitch) — ~180★ **Windows-only** (C++/Win32/WebView2).
- [`jslorrma/litellm-codex-oauth-provider`](https://github.com/jslorrma/litellm-codex-oauth-provider) — single-account LiteLLM provider; would need LiteLLM Router + N instances; experimental.

---

## Summary table — what we borrowed and what we left

| Project | Stars | Borrowed | Left |
|---|---|---|---|
| caam | 124 | regex 429 detector, scoring formula, retry-with-rotation harness shape | vault swap, daemon, multi-tool |
| ndycode/codex-multi-auth | 271 | diagnostics surface, quota cache TTL | renamed binary, loopback proxy, plugin infra |
| Loongphy/codex-auth | 1,757 | wham/usage call pattern | shell-out dep |
| Loongphy/codext | 80 | (nothing — hot-reload redundant w/ persistent CODEX_HOME) | fork maintenance |
| Soju06/codex-lb | 1,496 | dashboard idea (post-merge follow-up) | wholesale (wrong shape) |
| prakersh/cma | 22 | — | encrypted vault, GPL-3.0 |
| Ducksss/codex-profiles | 1 | per-`CODEX_HOME` isolation pattern (validation) | — |
| Spielewoy/multi-codex | 65 | per-`CODEX_HOME` isolation pattern (validation) | — |
| wakamex/codex-cli-usage | — | (reference: alternative `/codex/usage` endpoint) | — |
| Liam-Deacon/codex-usage | 2 | configurable-threshold validation | — |

---

## Unique novelty in this design

**No reviewed project solves the `(account × group_id)` two-axis problem.** caam, ndycode, Loongphy, codex-profiles, multi-codex, prakersh all collapse to one axis (account, config_recipe, or project). The group axis (per-terminal / per-PPID stable id) is unique to codex-session because we're the only wrapper acting as a transparent shim for headless agent flows (`/prex`, `/review-loop`). ADR [D2](04-decisions.md) documents the rationale.
