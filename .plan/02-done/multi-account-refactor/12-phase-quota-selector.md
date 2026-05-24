# Round 3 — Quota reader + scoring selector

This file is the **prex input** for Round 3. Pass its contents verbatim to `/prex -ar`.

---

## Prerequisite

Rounds 1 and 2 have landed. `codex-session account add/list/current/use/remove` work; `--account <name>` routes correctly; `--account auto` currently warns and falls through to pinned/default.

## Goal

Build the inline Rust HTTP client for `wham/usage`, a defensive parser, a 30 s TTL cache, and a caam-borrowed scoring selector. Wire `--account auto` to the selector. Add `account quota [--live] [--json]` sub-verb under `account`.

## Background (read before planning)

- `.plan/multi-account-refactor/06-quota-protocol.md` — full spec (endpoint, headers, parsing rules, cache schema, CLI surface).
- `.plan/multi-account-refactor/07-failover-spec.md` — (Round 4 territory, but the cooldown file schema is also read by the selector; selector treats `cooldown.json` as disqualifying).
- `.plan/multi-account-refactor/04-decisions.md` — ADRs D5 (inline Rust quota reader), D7 (caam scoring formula).
- `.plan/multi-account-refactor/03-inspired-projects.md` — caam scoring formula source citation + Loongphy/codex-auth wham/usage source.
- `.plan/multi-account-refactor/02-references.md` — F5 (quota endpoint), F6 (write-back race — sidestepped by per-account `CODEX_HOME`).
- `https://www.knightli.com/en/2026/04/12/codex-usage-quota-check/` — quota endpoint shape + defensive-parse warning.
- caam scoring source: `raw.githubusercontent.com/Dicklesworthstone/coding_agent_account_manager/refs/heads/main/internal/rotation/rotation.go` (read this; the formula is verbatim).

## Numbered implementation steps

1. **Add dependencies to `Cargo.toml`.**
    ```toml
    [dependencies]
    reqwest = { version = "0.12", default-features = false, features = ["blocking", "rustls-tls", "json"] }

    [dev-dependencies]
    wiremock = "0.6"
    ```
    - Confirm `cargo deny` + `cargo audit` accept the new transitive deps. May need a `deny.toml` license addition (`MPL-2.0` for rustls; `Apache-2.0` already common). Run `just deny` after the add to surface any new license needing approval.
    - `rustls-tls` strongly preferred over `native-tls` — no OpenSSL link needed.

2. **New `src/services/account/quota.rs`.**
    - Public surface:
      ```rust
      pub(crate) enum QuotaResult { Ok(Quota), ApiKeyMode, Stale(Quota) }
      pub(crate) struct Quota { pub five_hour: Window, pub weekly: Window }
      pub(crate) struct Window { pub percent_left: f64, pub reset_at_unix: u64 }

      pub(crate) fn get(account: &AccountId, ttl: Duration) -> Result<QuotaResult, QuotaError>;
      pub(crate) fn refresh(account: &AccountId) -> Result<QuotaResult, QuotaError>;
      ```
    - `get` reads cache, returns if fresh, else calls `refresh`.
    - `refresh`:
      - Loads the account's `auth.json` (location per R2 design — likely `<state>/accounts/<name>/auth.json` or the first group's auth).
      - Branches on shape: ChatGPT-plan OAuth (`tokens.access_token` + `tokens.account_id`) → call endpoint; API-key-only → return `ApiKeyMode` and skip the HTTP call.
      - GET `https://chatgpt.com/backend-api/wham/usage` with headers per `06-quota-protocol.md`.
      - One retry with 1 s backoff on `5xx`; surface as `QuotaError::HttpStatus(N)` on second failure.
    - **Defensive parser** (per `06-quota-protocol.md`):
      - Try `rate_limit` key, then `rate_limits`.
      - For each window: try `five_hour` → `primary_window`; try `weekly` → `secondary_window`.
      - Parse only raw numeric fields: `percent_left: f64`, `reset_at_unix: u64` (from `reset_time_ms / 1000` preferred, ISO-8601 `reset_at` fallback).
      - Tolerate unknown extra keys (`serde_json::Value` for permissive parsing, or `#[serde(default)]` + ignore-unknown).
      - Reject only if neither `rate_limit` nor `rate_limits` is present, or if either window is missing.
    - Cache file: `<state>/cache/quota/<account>.json`. Atomic write via `src/adapters/fs.rs`. Read-modify-write is fine — no concurrent writers expected from a single process.
    - **`QuotaError` enum** with variants `Network(reqwest::Error)`, `HttpStatus(u16)`, `ParseMissingRateLimit`, `ParseMissingWindow(&'static str)`, `AuthMissing(AuthError)`. All map to `AppError::Account(AccountError::QuotaFetch | QuotaParse)` via `From`.

3. **New `src/services/account/selector.rs`.**
    - Public surface:
      ```rust
      pub(crate) fn pick(ctx: &AppContext) -> Result<AccountId, AccountError>;
      ```
    - Iterate all registered accounts via `registry::list()`.
    - For each:
      - Check cooldown: if `<state>/accounts/<name>/cooldown.json` exists and `reset_at_unix > now` → **disqualified**, skip. (Cooldown file is written by R4; in R3 it just doesn't exist, so nothing is disqualified by cooldown.)
      - Fetch quota: `quota::get(name, ttl=30s)`. If `Err` → log warn, omit availScore but include account (still eligible).
      - Apply caam scoring (formula verbatim from D7):
        - `health_bonus` = `+100` healthy / `+50` degraded / `-50` unhealthy. (For R3, default `health = healthy` for all accounts — health tracking is not in scope. Score = +100 for everyone on this axis.)
        - `penalty` = 0 default (no penalty tracking yet — R4 may add).
        - `plan_bonus` = read from `auth.json` if surfaced; else 0. The plan tier is sometimes in `tokens.plan` or similar. **Defensive: if not present, default 0.**
        - `recency`:
          - Read LRU from `<state>/state/last-account`.
          - If this account == LRU → `-30` (recently used; penalize).
          - Else if account's `last_used_at` (`mtime` of `<account>/groups/`) > N days ago (default 7) → `+20` (long-idle; reward).
          - Else `0`.
        - `avail_score` = (quota OK?) → `(percent_left_five_hour + percent_left_weekly) / 2 - 50`. (If quota errored or `ApiKeyMode`, omit.)
        - `weekly_pressure_penalty` = if `weekly.percent_left < 20.0` → `-30`; else `0`.
        - **Threshold gate**: account is eligible only if `five_hour.percent_left > 50.0 && weekly.percent_left > floor` (floor default 10.0, override via `Config.account.weekly_floor`). API-key accounts skip the gate (always eligible).
        - Total score = sum of components.
      - Track best-scoring eligible account.
    - Return best, or `Err(AccountError::NoEligible)` if none qualify.
    - On success, write LRU: `registry::set_current(picked)`.

4. **Add `auto` arm to `src/services/account/resolver.rs`.**
    - When `AccountSelector::Auto` is encountered, call `selector::pick(ctx)`. Replaces the R2 "warn and fall through" stub.
    - Update the R2 warning test (`auto_warns_in_r2`) — it now ASSERTS the selector runs; rename to `auto_invokes_selector`.

5. **Add `src/commands/account/quota.rs` (new sub-verb).**
    - `pub(crate) fn run(ctx, args: &QuotaArgs) -> Result<(), AppError>`.
    - `QuotaArgs { live: bool, json: bool, account: Option<AccountId>, all: bool }` (in `cli/account.rs` alongside the existing sub-verbs).
    - Modes:
      - Default: cached, current account.
      - `--live`: forces refresh.
      - `--all`: iterate registered accounts.
      - `--account <name>`: query named account.
      - `--json`: emit structured JSON per `06-quota-protocol.md`.
    - Default text output per `06-quota-protocol.md`:
      ```
      account: work (active)
      five-hour:  73.4% left, resets in 2h 14m
      weekly:     87.1% left, resets in 3d 5h
      fetched:    18 s ago (cached, TTL 30s)
      ```
    - Register the new sub-verb in `src/cli/account.rs::AccountSubcommand::Quota(QuotaArgs)` and route in `src/commands/account/mod.rs::dispatch`.

6. **New error variants in `src/error.rs`.**
    - `AccountError::QuotaFetchFailed(QuotaError)` → exit code 69 (`EX_UNAVAILABLE`).
    - `AccountError::QuotaParseFailed(String)` → exit code 65 (`EX_DATAERR`).
    - `AccountError::NoEligible` → exit code 75 (`EX_TEMPFAIL`).
    - Per-variant `.kind()` strings for structured logging.

7. **Config additions in `src/config/mod.rs`.**
    - `AccountConfig` gains:
      - `quota_ttl_secs: u64` (default 30).
      - `weekly_floor: f64` (default 10.0).
      - `five_hour_threshold: f64` (default 50.0).
    - Env mirrors: `CODEX_SESSION_ACCOUNT_QUOTA_TTL_SECS`, etc.

8. **Tests.**
    - `tests/account_quota_basic.rs` — `wiremock` server returns canned `rate_limit` payload → `quota::get` parses correctly; happy path for each shape variant (rate_limit / rate_limits, five_hour / primary_window, etc.).
    - `tests/account_quota_defensive.rs` — missing fields, unknown extra keys, ISO-8601 vs ms, schema drift cases per `06-quota-protocol.md` table.
    - `tests/account_quota_cache.rs` — TTL respected (no second wiremock call), `--live` forces refresh, cache file atomically written, malformed cache file is overwritten on next fetch.
    - `tests/account_quota_apikey.rs` — auth.json without `tokens.access_token` returns `ApiKeyMode`; no HTTP call.
    - `src/services/account/selector.rs::tests` (unit) — table-driven scoring:
      - 3 accounts with varying quota → picks highest score.
      - Account below threshold → excluded.
      - All below threshold → `NoEligible`.
      - One in cooldown → excluded even if score would win.
      - API-key account → eligible, score from non-quota components.
      - LRU penalty + long-idle reward applied correctly.
    - `tests/account_quota_cli.rs` — `codex-session account quota`, `--live`, `--json`, `--all`, `--account <name>` end-to-end.
    - `tests/account_auto_selector.rs` — `--account auto exec ...` with 2 accounts: one healthy, one above threshold → picks healthy; both below → `NoEligible`.

## Files touched (representative)

- `Cargo.toml` (reqwest + wiremock + maybe deny.toml license tweak)
- `src/services/account/{quota,selector}.rs` (NEW)
- `src/services/account/resolver.rs` (extend with `auto` arm)
- `src/commands/account/quota.rs` (NEW)
- `src/cli/account.rs` (register Quota sub-verb)
- `src/config/mod.rs`, `src/config/error.rs` (new fields)
- `src/error.rs` (new error variants + exit codes)
- `src/services/account/mod.rs` (re-exports)
- `tests/account_quota_*.rs` (~4 new files), `tests/account_auto_selector.rs` (NEW)
- `src/services/account/selector.rs::tests` (unit-style, in-file)

**Net LOC estimate:** ~600–750 (incl. ~250 LOC of tests). **New tests:** ~12–15.

## Done criteria

```sh
just precommit-all     # must exit 0
just deny              # must exit 0 with new deps
just audit             # must exit 0
```

Plus manual smoke against a real account:

```sh
codex-session account quota --live --json | jq .           # raw wham/usage shape
codex-session account quota                                  # cached (30s TTL)
codex-session --account auto exec "echo balanced"            # selector picks highest-scoring eligible
codex-session account quota --all                            # table of all accounts
```

## Out of scope for Round 3

- Reactive 429 failover / retry-with-rotation (Round 4).
- Cooldown file writing (Round 4) — selector READS cooldown.json in R3, but no code writes it yet.
- `account cooldown` sub-verb (Round 4).
- Deleting `auth/watcher.rs`, `auth/signal.rs` (Round 4).
- Health-tracking telemetry (post-merge follow-up; for R3 health defaults to "healthy" for all).
- Plan-tier auto-detection from auth.json (defensive: defaults to 0 plan_bonus if not surfaced).

## Constraints

- Use `just` recipes for verification.
- Blocking HTTP client only — no tokio runtime.
- Defensive parsing per `06-quota-protocol.md`. Raw numeric fields, not derived labels (knightli warning).
- Cache TTL configurable, default 30 s.
- caam scoring formula verbatim — cite the source URL in `selector.rs` doc comment.
- New structured-log `op=` keys: `quota.fetch`, `quota.cache_hit`, `quota.cache_miss`, `account.select`, `account.select_no_eligible`.
- HTTP errors map to typed `QuotaError` first, then `AppError::Account(AccountError::QuotaFetchFailed)` at the boundary.
