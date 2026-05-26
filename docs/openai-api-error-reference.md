# OpenAI API Error Reference

- **Last verified:** 2026-05-26

## 1. HTTP Status Codes

| Status | Meaning | Typical shape |
|---|---|---|
| `401 Unauthorized` | Missing/expired/invalid credentials. | `{"error":{"message":"...","type":"...","code":"..."}}` |
| `403 Forbidden` | Authenticated but not permitted for resource/action. | `{"error":{"message":"...","type":"...","code":"..."}}` |
| `429 Too Many Requests` | Rate limit or quota exhaustion. | `{"error":{"message":"...","type":"...","code":"..."}}` |

## 2. Rate-limit Patterns (`failover.rs`)

| Regex | Name | Example stderr line |
|---|---|---|
| `(?i)rate.?limit` | `rate-limit` | `Error: rate limit exceeded` |
| `(?i)quota.?exceeded` | `quota-exceeded` | `quota exceeded; retry after 300 s` |
| `\b429\b` | `429` | `HTTP 429 Too Many Requests` |
| `(?i)too.?many.?requests` | `too-many-requests` | `HTTP 429 Too Many Requests` |
| `(?i)exceeded.*rate` | `exceeded-rate` | `Error: exceeded rate quota` |
| `(?i)slow.?down` | `slow-down` | `Server says slow-down` |

## 3. Auth-failure Patterns (`failover.rs`)

| Regex | Name | Example stderr line |
|---|---|---|
| `\b401\b` | `401` | `HTTP 401 Unauthorized` |
| `(?i)unauthorized` | `unauthorized` | `Error: Unauthorized access` |
| `(?i)invalid.?auth` | `invalid-auth` | `invalid auth token` |
| `(?i)no.?auth.?credentials` | `no-auth-credentials` | `no auth credentials found` |
| `(?i)invalid.?grant` | `invalid-grant` | `error: invalid_grant` |
| `(?i)token.?exchange.?error` | `token-exchange-error` | `token exchange error` |
| `(?i)insufficient.?permissions` | `insufficient-permissions` | `insufficient permissions for this action` |

## 4. OAuth Refresh Errors

| Error | Meaning | Typical cause |
|---|---|---|
| `invalid_grant` | Refresh grant rejected by auth server. | Expired/revoked/invalid refresh token. |
| `refresh_token_reused` | Rotated refresh token was used again. | Concurrent refresh attempts or stale token state. |

## 5. Three-layer Defense Model

1. **Layer 1 — JWT pre-check (`selector.rs`)**: skip accounts whose token expires within 60 seconds before quota fetch.
2. **Layer 2 — Quota-level 401 retry (`quota.rs`)**: on quota-fetch 401, refresh token and retry once.
3. **Layer 3 — Exec-level detection (`retry.rs` + `failover.rs`)**: scan child stderr/stdout for auth-failure patterns; refresh same account once; cooldown + rotate on refresh failure.

## 6. Sources

- OpenAI API error codes: <https://platform.openai.com/docs/guides/error-codes>
- OAuth 2.0 refresh token grant: <https://www.rfc-editor.org/rfc/rfc6749#section-6>
- OpenAI Codex discussions/issues: <https://github.com/openai/codex/issues/4432>, <https://github.com/openai/codex/issues/10332>
