# 06 — Quota Protocol

Detailed spec for the `wham/usage` HTTP call. Implements R3's `src/services/account/quota.rs` and the cache at `<state-root>/cache/quota/<account>.json`.

## Endpoint

```
GET https://chatgpt.com/backend-api/wham/usage
```

Undocumented internal endpoint. Schema has changed between versions. **Treat as unstable.** Codex CLI itself polls this roughly every 60 s while the TUI is running ([openai/codex#10869](https://github.com/openai/codex/issues/10869)).

## Required headers

```
Authorization: Bearer <access_token>
ChatGPT-Account-Id: <account_id>
Accept: application/json
Origin: https://chatgpt.com
Referer: https://chatgpt.com/
User-Agent: Mozilla/5.0
```

`<access_token>` and `<account_id>` are both extracted from the account's `auth.json` file at `<state-root>/accounts/<account>/groups/<group-id>/auth.json` (for the active group) or any group's `auth.json` (the OAuth credential is account-scoped, not group-scoped).

## Auth.json fields

```json
{
  "OPENAI_API_KEY": "...",
  "tokens": {
    "id_token": "...",
    "access_token": "...",            ← Bearer
    "refresh_token": "...",
    "account_id": "...",              ← ChatGPT-Account-Id (when ChatGPT-plan)
    "expires_at": "..."
  },
  "last_refresh": "..."
}
```

The quota reader needs to handle both shapes:

- **ChatGPT-plan OAuth:** `tokens.access_token` + `tokens.account_id` present → quota check works.
- **API-key mode:** only `OPENAI_API_KEY` present → call still fires (per [#10869](https://github.com/openai/codex/issues/10869)) but we should detect and skip; emit "quota not available for API-key accounts" warning. Return `Quota::ApiKeyMode` (a typed marker) rather than error, so the selector can treat API-key accounts as "always eligible" without quota gating.

## Response shape (varies)

The response carries either `rate_limit` or `rate_limits` (older / newer naming). Each contains windowed sub-objects:

```jsonc
{
  "rate_limit": {                          // OR "rate_limits"
    "five_hour": {                          // OR "primary_window"
      "percent_left": 73.4,                 // raw float, 0..100
      "used_percent": 26.6,                 // raw float
      "reset_at": "2026-05-22T18:00:00Z",   // ISO-8601 OR
      "reset_time_ms": 1716393600000,       // unix ms
      "limit_window_seconds": 18000
    },
    "weekly": {                              // OR "secondary_window"
      "percent_left": 87.1,
      "used_percent": 12.9,
      "reset_at": "2026-05-26T00:00:00Z",
      "limit_window_seconds": 604800
    },
    // Per-model windows (optional, not consumed by R3):
    "models": { ... },
    "code_review": { ... }
  }
}
```

## Parsing rules (defensive — knightli warning)

**Knightli explicitly warns: "read raw fields, not derived labels."** The response shape has already changed between versions.

Rules:

1. **Accept either `rate_limit` or `rate_limits`.** Try the singular first; fall back to plural.
2. **Accept either `five_hour` or `primary_window` for the first window; either `weekly` or `secondary_window` for the second.**
3. **Parse only the raw numeric fields:** `percent_left`, `reset_at` / `reset_time_ms`, `limit_window_seconds`.
4. **Tolerate unknown extra keys at any level** — `serde_json` with `#[serde(deny_unknown_fields)]` would be wrong here. Use the default permissive mode.
5. **Reject** with `QuotaParseFailed` only if neither `rate_limit` nor `rate_limits` is present at the top level (or both windows are missing).
6. **Normalize `reset_at` to unix seconds** in our internal `Window { percent_left: f64, reset_at_unix: u64 }`. Prefer `reset_time_ms` → `/1000` if both present; fall back to ISO-8601 parsing.

### Rust shape (sketch)

```rust
pub struct Quota {
    pub five_hour: Window,
    pub weekly: Window,
}

pub struct Window {
    pub percent_left: f64,
    pub reset_at_unix: u64,
}

pub enum QuotaResult {
    Ok(Quota),
    ApiKeyMode,                       // detected from auth.json shape
    Err(QuotaError),
}

pub enum QuotaError {
    Network(reqwest::Error),
    HttpStatus(u16),                  // non-2xx
    ParseMissingRateLimit,
    ParseMissingWindow(&'static str), // "five_hour"|"weekly"
}
```

## Cache

```
<state-root>/cache/quota/<account>.json
```

```json
{
  "fetched_at_unix": 1716393600,
  "body": {
    "five_hour": { "percent_left": 73.4, "reset_at_unix": 1716393600 },
    "weekly":    { "percent_left": 87.1, "reset_at_unix": 1716998400 }
  }
}
```

- **TTL:** default 30 s. Configurable via `Config.account.quota_ttl_secs`.
- **`quota::get(account, ttl) -> Result<QuotaResult, QuotaError>`** — returns cached if fresh, else refetches and writes through.
- **`quota::refresh(account) -> Result<QuotaResult, QuotaError>`** — always fetches; bypasses cache. Wired to `account quota --live`.
- **Cache writes are atomic** via `src/adapters/fs.rs::atomic_write`.
- **`ApiKeyMode` is cached too** with a longer TTL (5 min) to avoid pointlessly calling the endpoint for API-key accounts on every selector run.

## Selector consumption (R3)

The selector reads cached quota via `quota::get(account, ttl=30s)`. If cache miss → blocks on a refetch (the user opted into `--account auto`, they're willing to pay the round-trip).

If `quota::get` returns `Err`:

- Treat as "quota unknown" — account is **not** disqualified, but loses the `availScore` contribution to its score. Selector picks based on the remaining factors (health, recency, plan).
- Log `op="quota.fetch" status="error" err.kind=<...>` at warn level.

If `ApiKeyMode`: account is treated as "always eligible"; no quota gate.

## HTTP client choice

`reqwest` blocking with `features = ["blocking", "rustls-tls", "json"]`. Reasons:

- **Blocking, not async** — codex-session is sync top-to-bottom; no tokio runtime exists; adding one would balloon the binary.
- **`rustls-tls`, not `native-tls`** — avoids the OpenSSL link; builds cleanly in containers without `openssl-dev`.
- **`json` feature** — gives `Response::json::<T>()` for serde integration.

`Cargo.toml` addition:

```toml
[dependencies]
reqwest = { version = "0.12", default-features = false, features = ["blocking", "rustls-tls", "json"] }

[dev-dependencies]
wiremock = "0.6"
```

## Test strategy (R3)

`tests/account_quota_*.rs` uses `wiremock` to stand up a local HTTP server returning canned responses:

| Test | Fixture |
|---|---|
| Happy path with `rate_limit` shape | sample payload with `rate_limit.five_hour` + `rate_limit.weekly` |
| New shape with `rate_limits` plural | swap key name |
| Old `primary_window` / `secondary_window` aliases | swap window key names |
| Missing `reset_time_ms`, present `reset_at` (ISO-8601) | parse from string |
| Both `reset_time_ms` and `reset_at` (prefer ms) | numerical preference |
| Unknown extra top-level keys | tolerate |
| HTTP 401 | `QuotaError::HttpStatus(401)` |
| HTTP 500 | retry with one backoff (~1 s); on second failure `QuotaError::HttpStatus(500)` |
| Network error (connection refused) | `QuotaError::Network` |
| Empty body | `QuotaError::ParseMissingRateLimit` |
| Body with `rate_limit` but missing `five_hour` | `QuotaError::ParseMissingWindow("five_hour")` |
| API-key-only `auth.json` | skip call entirely, return `ApiKeyMode` |
| Cache hit within TTL | no HTTP call recorded by wiremock |
| Cache miss past TTL | refetches; writes through atomically |

## CLI surface

```sh
codex-session account quota                     # cached, current account
codex-session account quota --live              # force refresh, current account
codex-session account quota --json              # raw parsed shape
codex-session account quota --live --json
codex-session account quota --account work
codex-session account quota --all               # iterate registry
codex-session account quota --all --json
```

Default output (non-JSON, current account, cached):

```
account: work (active)
five-hour:  73.4% left, resets in 2h 14m
weekly:     87.1% left, resets in 3d 5h
fetched:    18 s ago (cached, TTL 30s)
```

`--all` adds one line per registered account, ranked by `five_hour.percent_left` descending.

`--json` emits a single object (or array, for `--all`) with the cached payload + `fetched_at_unix` for downstream tooling.
