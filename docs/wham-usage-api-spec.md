# WHAM Usage API Specification

How `codex-session` fetches and parses rate-limit quota from the OpenAI
WHAM usage endpoint. This is an undocumented internal endpoint whose
schema has changed between versions — **treat as unstable.**

- **Last verified:** 2026-05-26
- **Source file:** `src/services/account/quota.rs`
- **Maintenance:** if a quota parse failure appears (`missing window`,
  `missing rate_limit`), the response shape may have changed again.
  Run the **Re-verification recipe** (§7) and update this file.

---

## 1. Endpoint

```
GET https://chatgpt.com/backend-api/wham/usage
```

Override: set `CODEX_SESSION_WHAM_USAGE_URL` to redirect requests to a
different base URL (used by tests and development).

## 2. Request headers

| Header | Value | Notes |
|---|---|---|
| `Authorization` | `Bearer <access_token>` | From `auth.json` `tokens.access_token` |
| `ChatGPT-Account-Id` | `<account_id>` | From `auth.json` `tokens.account_id` |
| `Accept` | `application/json` | |
| `Origin` | `https://chatgpt.com` | |
| `Referer` | `https://chatgpt.com/` | |
| `User-Agent` | `Mozilla/5.0` | |

## 3. Auth modes

- **OAuth (ChatGPT plan):** `tokens.access_token` + `tokens.account_id`
  present in `auth.json` → quota fetch proceeds.
- **API-key mode:** only `OPENAI_API_KEY` present → fetch is skipped;
  `QuotaResult::ApiKeyMode` is returned and cached for 5 min.

## 4. Response shapes

### 4a. Legacy shape (pre-2026-05)

```jsonc
{
  "rate_limit": {                          // OR "rate_limits"
    "five_hour": {
      "percent_left": 73.4,               // 0–100
      "reset_time_ms": 1716393600000,      // unix ms
      // OR "reset_at": "2026-05-22T18:00:00Z"  (ISO-8601)
      "limit_window_seconds": 18000
    },
    "weekly": {
      "percent_left": 87.1,
      "reset_time_ms": 1716998400000,
      "limit_window_seconds": 604800
    }
  }
}
```

Aliases observed in older versions:
- `primary_window` (alias for `five_hour`)
- `secondary_window` (alias for `weekly`)

### 4b. Current shape (2026-05, verified live)

```jsonc
{
  "user_id": "...",
  "account_id": "...",
  "plan_type": "team",                     // or "pro", "enterprise", etc.
  "rate_limit": {
    "allowed": true,
    "limit_reached": false,
    "primary_window": {
      "used_percent": 1,                   // 0–100, integer or float
      "limit_window_seconds": 18000,
      "reset_after_seconds": 18000,
      "reset_at": 1779813200               // unix seconds (integer, NOT ISO-8601)
    },
    "secondary_window": {
      "used_percent": 0,
      "limit_window_seconds": 604800,
      "reset_after_seconds": 604800,
      "reset_at": 1780400000
    }
  },
  "code_review_rate_limit": null,
  "additional_rate_limits": null,
  "credits": { ... },
  "spend_control": { ... }
}
```

## 5. Field name mappings (parser priority order)

The parser tries field names left-to-right; the first match wins.

### 5a. Root key

| Try | Notes |
|---|---|
| `rate_limit` | Legacy singular |
| `rate_limits` | Current plural |

### 5b. Window names

| Internal name | Try in order |
|---|---|
| `five_hour` | `five_hour` → `primary_window` → `primary` |
| `weekly` | `weekly` → `secondary_window` → `secondary` |

### 5c. Percent field

| Internal field | Try in order | Conversion |
|---|---|---|
| `percent_left` | `percent_left` → `used_percent` → `usedPercent` | identity / `100 − used` / `100 − used` |

### 5d. Reset time field

| Internal field | Try in order | Conversion |
|---|---|---|
| `reset_at_unix` | `reset_time_ms` → `resetsAt` → `reset_at` | ms ÷ 1000 / seconds / integer-or-RFC-3339 |

**Note on `reset_at`:** the current API sends `reset_at` as a unix-seconds
integer, not an ISO-8601 string. The parser tries integer first, then falls
back to RFC-3339 string parsing for backward compatibility.

## 6. Error behavior

- **Missing root key** (`rate_limit` / `rate_limits`): `QuotaError::ParseMissingRateLimit`, exit 65.
- **Missing window**: `QuotaError::ParseMissingWindow("five_hour")` or `("weekly")`, exit 65.
- **HTTP 5xx**: retry once after 1 s; on second failure, `QuotaError::HttpStatus`, exit 69.
- **HTTP 401**: triggers an OAuth token refresh attempt (see below);
  on success, retries the WHAM request once.  If the refresh also fails,
  surfaces `QuotaError::HttpStatus(401)`, exit 69.
- **HTTP 4xx (non-401)**: immediate `QuotaError::HttpStatus`, exit 69.
- **Network error**: `QuotaError::Network`, exit 69.
- **Stale cache fallback**: on fetch error, if a cache entry exists (any age), return
  `QuotaResult::Stale` rather than failing.

### 6.1 Token refresh on 401

When the WHAM endpoint returns HTTP 401, the quota module attempts an
OAuth token refresh before giving up:

1. Read the `refresh_token` from the same auth file used for the original request.
2. `POST https://auth.openai.com/oauth/token` with `grant_type=refresh_token`, `client_id=app_EMoamEEZ73f0CkXaXp7hrann`.
3. On success: save the new `access_token` + `refresh_token` back to the auth file, then retry the WHAM request once.
4. On failure: surface the original 401 as `QuotaError::HttpStatus`.

Override for testing: `CODEX_SESSION_TOKEN_ENDPOINT` env var.

See `docs/openai-oauth-token-lifecycle.md` for the full token rotation
behavior.

## 7. Re-verification recipe

Fetch the live response and inspect the structure:

```bash
# Requires: valid OAuth access_token and account_id from auth.json.
# Find your auth.json:
#   ls ~/.local/state/codex-session/accounts/*/groups/*/auth.json

ACCESS_TOKEN="$(jq -r '.tokens.access_token' <AUTH_JSON_PATH>)"
ACCOUNT_ID="$(jq -r '.tokens.account_id' <AUTH_JSON_PATH>)"

curl -s \
  -H "Authorization: Bearer $ACCESS_TOKEN" \
  -H "ChatGPT-Account-Id: $ACCOUNT_ID" \
  -H "Accept: application/json" \
  -H "Origin: https://chatgpt.com" \
  -H "Referer: https://chatgpt.com/" \
  -H "User-Agent: Mozilla/5.0" \
  "https://chatgpt.com/backend-api/wham/usage" | jq .
```

Compare the output against §4 and §5 above. If the shape has changed:

1. Update §4 with the new shape.
2. Update §5 field-name mappings if new aliases appeared.
3. Update `parse_quota_body()` / `parse_window()` /
  `parse_reset_at_unix()` in `src/services/account/quota.rs`.
4. Add new wiremock fixtures in `tests/account_quota_basic.rs`.
5. Bump the `Last verified` date at the top of this file.

Automated: `just test-live` runs live integration tests that parse the
real API response and fail with a pointer to this file if the shape is
no longer recognized.

## 8. Sources

All sources accessed 2026-05-26.

- [OpenAI Codex App Server docs](https://developers.openai.com/codex/app-server) —
  documents `account/rateLimits/read` JSON-RPC endpoint with `primary`/`secondary`
  window naming and `usedPercent`/`resetsAt` fields.
- [GitHub openai/codex #14880](https://github.com/openai/codex/issues/14880) —
  `rate_limits` field null in rollout session files after GPT-5.4 release (2026-03-17).
- [GitHub openai/codex #24445](https://github.com/openai/codex/issues/24445) —
  rate limits desync with null fields.
- [OpenAI Community: understanding the new Codex limit system](https://community.openai.com/t/understanding-the-new-codex-limit-system-after-the-april-9-update/1378768) —
  confirms restructuring from per-message to token-based usage (2026-04).
- [OpenAI Community: Codex rate limits reset for all paid plans](https://community.openai.com/t/codex-rate-limits-reset-for-all-paid-plans-april-28-2026/1379921) —
  confirms rate limit window changes (2026-04-28).
- [OpenAI Codex Changelog](https://developers.openai.com/codex/changelog) —
  tracks endpoint and pricing changes.
- [openai/codex #10869](https://github.com/openai/codex/issues/10869) —
  Codex CLI polling frequency for wham/usage (~60 s).
