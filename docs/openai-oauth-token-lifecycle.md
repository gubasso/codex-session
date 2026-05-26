# OpenAI OAuth Token Lifecycle

Empirically verified behavior of OpenAI's OAuth token system as used by
the Codex CLI (`codex-rs`).  This document records facts relevant to
`codex-session`'s multi-account architecture — consult it before making
changes to auth flows.

- **Last verified:** 2026-05-26
- **Codex version checked:** `codex-cli 0.132.0`
- **Client ID:** `app_EMoamEEZ73f0CkXaXp7hrann` (embedded in the codex
  binary; public PKCE client)

---

## 1. Token types

| Token | Format | TTL | Storage |
|---|---|---|---|
| `access_token` | JWT (RS256) | ~10 days (`exp` claim) | `$CODEX_HOME/auth.json` → `tokens.access_token` |
| `refresh_token` | Opaque (`rt_…`) | Long-lived, single-use | `$CODEX_HOME/auth.json` → `tokens.refresh_token` |
| `id_token` | JWT | Same as access | `$CODEX_HOME/auth.json` → `tokens.id_token` |
| `account_id` | UUID | N/A | `$CODEX_HOME/auth.json` → `tokens.account_id` |

The `access_token` JWT contains (among others): `sub` (user ID, e.g.
`auth0|66bcc…`), `aud` (`["https://api.openai.com/v1"]`), `sid`
(session ID, `authsess_…`), `iat`, `exp`.

## 2. Token refresh endpoint

```
POST https://auth.openai.com/oauth/token
Content-Type: application/json

{
  "grant_type":    "refresh_token",
  "client_id":     "app_EMoamEEZ73f0CkXaXp7hrann",
  "refresh_token": "<old_refresh_token>"
}
```

**Response (success):**

```json
{
  "access_token":  "<new_jwt>",
  "refresh_token": "<new_opaque_rt>",
  "token_type":    "Bearer",
  "expires_in":    864000
}
```

Override for testing: set `CODEX_SESSION_TOKEN_ENDPOINT`.

## 3. Refresh token rotation (single-use)

OpenAI implements RFC 6749-compliant refresh token rotation:

- Every use of a `refresh_token` returns a **new** `access_token` AND a
  **new** `refresh_token`.
- The old `refresh_token` is **permanently invalidated** the moment it
  is exchanged.
- If the old `refresh_token` is used again, OpenAI returns:
  ```json
  {
    "error": {
      "message": "Your refresh token has already been used to generate a new access token. Please try signing in again.",
      "code": "refresh_token_reused"
    }
  }
  ```
- Token-family revocation: a reused refresh token may trigger
  invalidation of the entire token family (security measure against
  token theft).

**Implication:** after every successful refresh, the new `refresh_token`
must be persisted **immediately**.  If the write fails, the old token is
dead and the session requires a full re-login.

## 4. Token revocation endpoint

```
POST https://auth.openai.com/oauth/revoke
Content-Type: application/x-www-form-urlencoded

token=<refresh_token>&token_type_hint=refresh_token&client_id=app_EMoamEEZ73f0CkXaXp7hrann
```

- Returns 200 on success (even if the token was already invalid).
- Revoking a `refresh_token` also invalidates the associated
  `access_token` family.

## 5. Server-side session validation

Access tokens can be **rejected by the server before their JWT `exp`
date**.  The WHAM usage API (`chatgpt.com/backend-api/wham/usage`)
checks tokens against a server-side session registry, not just JWT
signature + expiry.  A revoked token returns HTTP 401 even though the
JWT is structurally valid.

## 6. Upstream codex CLI revocation behavior

### `codex logout` (PR #17825)

Sends the stored `refresh_token` to the revocation endpoint before
deleting local auth.  Fail-closed: if revocation fails, local auth is
preserved.

### `codex login` (PR #21747)

When re-logging in, codex revokes any previously-stored managed token
before saving the new one.  This prevents token accumulation on the
server.

### Combined effect on multi-account

If multiple `codex login` / `codex logout` cycles share the same
`$CODEX_HOME` (i.e. `~/.codex/`), **each cycle revokes the previous
account's token**.  Only the most recently authenticated account has a
valid token.  This is the root cause of the multi-account invalidation
bug that motivated `CODEX_HOME` isolation.

## 7. Empirical evidence (2026-05-26)

Three accounts (mari, cwnt, isma) — all in the same OpenAI organization
(`account_id: 25aca47e-96e6-4d70-b1d5-86ea0c0322d3`):

| Account | JWT expired? | Refresh token status | WHAM API |
|---|:---:|---|---|
| cwnt (last login) | No (10d TTL) | Valid | 200 OK |
| mari (previous) | No (10d TTL) | `refresh_token_reused` | 401 |
| isma (oldest) | No (10d TTL) | `refresh_token_reused` | 401 |

The fix: run each `codex login` / `codex logout` inside an ephemeral
`CODEX_HOME` (temp dir under `state_dir/auth-ops/`) so no auth
operation touches another account's token state.

## 8. Re-verification recipe

```bash
# Test refresh token validity for an account:
curl -s -X POST 'https://auth.openai.com/oauth/token' \
  -H 'Content-Type: application/json' \
  -d '{"grant_type":"refresh_token","client_id":"app_EMoamEEZ73f0CkXaXp7hrann","refresh_token":"<RT>"}' \
  | python3 -c "import json,sys; d=json.load(sys.stdin); print('OK' if 'access_token' in d else d)"
```

**Caution:** this _consumes_ the refresh token (rotation).  Only use
for diagnosis, and save the new token if it succeeds.

## Sources

- [OpenAI Codex Auth docs](https://developers.openai.com/codex/auth)
- [PR #17825 — Revoke ChatGPT tokens on logout](https://github.com/openai/codex/pull/17825)
- [PR #21747 — Revoke superseded auth tokens on relogin](https://github.com/openai/codex/pull/21747)
- [Issue #10332 — Race condition in OAuth token refresh](https://github.com/openai/codex/issues/10332)
- [Issue #4432 — First-class multi-account auth](https://github.com/openai/codex/issues/4432)
- [RFC 6749 — OAuth 2.0 Refresh Token Grant](https://tools.ietf.org/html/rfc6749#section-6)
