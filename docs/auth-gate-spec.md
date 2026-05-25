# Auth Gate Specification

> How `codex-session` resolves, validates, and authenticates accounts before
> every pass-through launch.

## 1. Overview

Every command that forwards to the upstream `codex` binary (bare
`codex-session`, `codex-session exec ...`, etc.) passes through an **auth
gate** before execution. The gate assesses the current account state,
determines whether the resolved account has a valid authentication seed, and
either proceeds, prompts the user interactively, or fails with an actionable
error message.

**Entry point:** `gate::ensure()` is called at the top of
`pass_through::run()`, before the dry-run path and the retry loop.

**Source file:** `src/services/account/gate.rs`

## 2. Authentication model

### 2.1 The three auth layers

Authentication tokens flow through three layers, each with a distinct
lifetime and scope:

```
Layer 1: Native auth          ~/.codex/auth.json
          │
          │  copy_native_auth_to_seed()
          ▼
Layer 2: Account seed         <state>/accounts/<name>/auth.json
          │
          │  materialize_account_auth_seed()
          ▼
Layer 3: Session group copy   <session-dir>/<group>/auth.json
```

| Layer | Path | Scope | Lifetime | Written by |
|---|---|---|---|---|
| Native | `~/.codex/auth.json` | Global singleton | Overwritten on every `codex login`, deleted on `codex logout` | `codex login` / `codex logout` |
| Account seed | `<state>/accounts/<name>/auth.json` | Per-account | Persists until `account refresh` or `account remove` | `copy_native_auth_to_seed()` |
| Session group copy | `<session-dir>/<group>/auth.json` | Per-session-group | Created once per group, reused across invocations | `materialize_account_auth_seed()` |

### 2.2 Why the account seed is the source of truth

The **native auth** (`~/.codex/auth.json`) is a global singleton — `codex
login` always overwrites it regardless of which account the user is
authenticating. It reflects only the *most recent* login, not the state of
any particular account. In a multi-account setup (accounts A, B, C), native
auth might reflect account B while accounts A and C still have valid
independent seeds.

The **account seed** (`accounts/<name>/auth.json`) is the per-account
snapshot captured at registration (`account add`) or renewal (`account
refresh`). It is the canonical record of whether a specific account has ever
been authenticated. The gate checks this file's existence and, for OAuth
tokens, validates the token against the server (see §2.3).

The **session group copies** (`groups/<gid>/auth.json`) are runtime
artifacts. They are materialized from the seed before each child invocation
and may accumulate stale copies across group IDs. The gate ignores them.

### 2.3 Why there is no server-side token probe

An earlier implementation included an HTTP probe that validated the
OAuth access token against `chatgpt.com/backend-api/me` on every
launch. This was removed because:

1. **Wrong endpoint.** The codex CLI obtains OAuth tokens scoped for
    `api.openai.com` (`api.connectors.*` scopes), but the probe hit the
    ChatGPT *web* backend. That endpoint returned 403 for valid CLI
    tokens, causing the gate to report `AuthMissing` on every launch —
    even when the account was freshly authenticated.

2. **Token refresh invalidation.** During a TUI session, the codex
    runtime may refresh the OAuth token. The refreshed token is written
    to the session group copy (`$CODEX_HOME/auth.json`), but the
    account seed still holds the original token. If the refresh revokes
    the original server-side, a probe against the seed would always
    fail. The `sync_group_auth_to_seed` mechanism (see §5) now
    propagates refreshed tokens back to the seed after each child exit,
    but the probe was still unreliable for the endpoint mismatch above.

3. **Redundant with managed logout.** The `codex-session logout`
    command deletes the account seed, so the gate correctly detects
    `AuthMissing` via seed-file existence alone. If a user runs native
    `codex logout` (bypassing codex-session), the codex runtime detects
    the invalid token at startup and surfaces an auth error — the user
    can then run `codex-session login` to re-authenticate.

**Current behavior:** The gate checks seed-file existence only. If the
seed exists, the account is `Ready`. If not, `AuthMissing`. No HTTP
request is made during `assess()`.

### 2.4 What the gate does NOT check

- **Token validity.** The gate does not validate the token against any
  server or parse expiry claims. It trusts seed-file existence.  If the
  token is invalid, the codex runtime detects the error at startup.

- **Group-level auth copies.** Stale session group dirs may contain old
  `auth.json` files from previous runs. These are not authoritative
  (though `sync_group_auth_to_seed` propagates refreshes after each
  child exit — see §5).

### 2.5 Auth lifecycle

**Registration (`account add`):**

1. `codex logout` — clear any previous native auth (non-fatal if it fails).
2. `codex login` — user authenticates in browser; codex writes
    `~/.codex/auth.json`.
3. `copy_native_auth_to_seed()` — reads `~/.codex/auth.json` with hardened
    file checks (ownership, mode, no symlinks, no hardlinks), writes a copy
    to `accounts/<name>/auth.json`.
4. `registry.set_current()` — sets the LRU pointer.

After step 3, the account seed exists and the gate will consider this
account `Ready`.

**Renewal (`account refresh`, `codex-session login`, or gate
re-authentication):**

1. `codex logout` + `codex login` — same as registration.
2. `copy_native_auth_to_seed()` — overwrites the existing seed with the
    fresh token.
3. `registry.delete_group_auths()` — removes all `groups/*/auth.json` so
    new sessions pick up the fresh seed instead of stale copies.

**Logout (`codex-session logout`):**

1. `codex logout` — revokes the token server-side (non-fatal if it
    fails).
2. `registry.delete_auth_seed()` — removes the account seed so the gate
    returns `AuthMissing` on the next invocation.
3. `registry.delete_group_auths()` — removes stale session copies.

After step 2, the account has no seed and the gate will guide the user
through re-authentication on next launch.

**Pass-through launch (every `codex-session exec`, bare invocation, etc.):**

1. `gate::ensure()` — assess + prompt/fail (see §3 below).
2. `materialize_account_auth_seed()` — if the session group dir does not
    already have an `auth.json`, copy the account seed into it. This is the
    file codex will read at `$CODEX_HOME/auth.json`.
3. Spawn child codex with `CODEX_HOME` pointing at the session group dir.
4. After child exits, `sync_group_auth_to_seed()` — see §5.

## 3. Gate assessment

### 3.1 State machine

`gate::assess()` evaluates the current state and returns one of four
variants:

<!-- editorconfig-checker-disable -->
```
                    ┌──────────────┐
                    │ registry.list│
                    └──────┬───────┘
                           │
                      empty?
                     ╱        ╲
                   yes         no
                   │            │
            ┌──────┴──────┐    resolver.resolve()
            │  NoAccounts │         │
            └─────────────┘    ┌────┴─────┐
                               │          │
                          NoneResolved   Ok(resolved)
                               │          │
                        ┌──────┴──────┐   expect_account_dir()
                        │ NoneSelected│      │
                        └─────────────┘ ┌────┴────┐
                                        │         │
                                   NotFound    Ok(dir)
                                      │          │
                               ┌──────┴──────┐  seed exists?
                               │ NoneSelected│  ╱       ╲
                               │(stale ptr)  │ yes       no
                               └─────────────┘ │         │
                                          ┌────┴───┐ ┌───┴────────┐
                                          │ Ready  │ │ AuthMissing│
                                          └────────┘ └────────────┘
```
<!-- editorconfig-checker-enable -->

| State | Condition | Meaning |
|---|---|---|
| `NoAccounts` | `registry.list()` is empty | Fresh install, no accounts ever registered |
| `NoneSelected` | Accounts exist but resolver returns `NoneResolved`, or resolved account's directory is missing (stale LRU/config pointer) | User has accounts but none could be resolved |
| `AuthMissing` | Account resolved and directory exists, but seed is absent | Account never authenticated, or seed deleted via `codex-session logout` |
| `Ready` | Account resolved, directory exists, seed present | Good to launch |

### 3.2 Resolution priority

The resolver (`resolver::resolve()`) tries sources in this order:

1. **Flag** — `--account <name>` or `--account auto`
2. **Env** — `CODEX_SESSION_ACCOUNT=<name>` or `=auto`
3. **Auto** — `selector::pick()` (quota-weighted scoring)
4. **LRU** — `state/last-account` pointer
5. **Config pinned** — `config.account.pinned`

If none matches, the resolver returns `Err(NoneResolved)`.

There are no implicit defaults or fallbacks — every resolved account traces
back to an explicit source.

## 4. Gate behavior by mode

### 4.1 Interactive mode (terminal attached)

| State | Behavior |
|---|---|
| `Ready` | Narrate account + source to stderr, proceed to launch. |
| `NoAccounts` | Narrate → prompt for account name → run `codex login` → save seed → launch. |
| `NoneSelected` | Narrate → `Select` menu: pick existing account or add new → launch. |
| `AuthMissing` | Warning to stderr → `Select` menu: re-authenticate / switch / add new → launch. |

Every step is narrated to stderr with a `[codex-session]` prefix so the user
always knows what is happening and why. Messages are suppressed under
`--silent`.

**AuthMissing example:**

```
warning: account 'work' is selected but has no valid authentication token.
The token may be missing or expired. You need to re-authenticate before launching.

? Account 'work' has no valid authentication. What would you like to do?
> Re-authenticate 'work'
  Switch to a different account
  Add a new account

[codex-session] re-authenticating account 'work' — running the codex native login flow...
[codex-session] logging out of any existing codex session first...
[codex-session] starting codex login — please authenticate in the browser to renew the token for account 'work'...
[codex-session] login succeeded — saving renewed authentication token...
[codex-session] clearing stale group auth tokens so new sessions use the fresh token...
[codex-session] account 'work' re-authenticated — launching.
```

### 4.2 Non-interactive mode (no terminal)

| State | Behavior |
|---|---|
| `Ready` | Proceed (narrate to stderr if not `--silent`). |
| `NoAccounts` | Hard error: `"no accounts registered; run codex-session account add <name>"` (exit 64). |
| `NoneSelected` | Hard error: `"accounts exist but none is selected; run codex-session account use <name>"` (exit 64). |
| `AuthMissing` | Hard error: `"account 'X' has no valid authentication; run codex-session account refresh X"` (exit 75). |

Every non-interactive error message includes the exact command to run to fix
the problem.

### 4.3 Explicit account flag

When the account source is **Flag** (`--account <name>`) or **Env**
(`CODEX_SESSION_ACCOUNT=<name>`), the gate trusts the explicit choice. It
still checks the seed and will error on `AuthMissing`, but it does not
prompt for confirmation even in interactive mode — the user already told us
what they want.

## 5. Managed login and logout

`codex-session login` and `codex-session logout` are intercepted in
`pass_through::run()` before the normal gate/retry path. They are
**not** raw pass-throughs to the native `codex` binary — they are
managed by the gate module and fully account-aware.

### 5.1 `codex-session login` (`gate::run_login`)

Calls `assess()` then dispatches:

| State | Interactive | Non-interactive |
|---|---|---|
| `NoAccounts` | Prompt for name, create account, authenticate | Error: `NoAccounts` |
| `NoneSelected` | Prompt to select, then authenticate | Error: `NoneSelected` |
| `AuthMissing` | `do_refresh_auth()` | `do_refresh_auth()` |
| `Ready` | `do_refresh_auth()` (force re-auth) | `do_refresh_auth()` |

Unlike `ensure()`, which returns immediately when `Ready`, `run_login()`
always forces a fresh authentication — the user explicitly asked to log in.

### 5.2 `codex-session logout` (`gate::run_logout`)

| State | Interactive | Non-interactive |
|---|---|---|
| `NoAccounts` | Narrate "nothing to log out", exit 0 | Same |
| `NoneSelected` | Prompt to select which to log out | Error: `NoneSelected` |
| `AuthMissing` | Narrate "already logged out", exit 0 | Same |
| `Ready` | `do_logout()` | `do_logout()` |

`do_logout()` runs native `codex logout` (non-fatal), deletes the
account seed via `registry.delete_auth_seed()`, and clears group auths.
After logout, `assess()` returns `AuthMissing` (seed absent).

### 5.3 Why login/logout are not raw pass-throughs

If `codex-session login` forwarded directly to `codex login`, the user
would authenticate against the global `~/.codex/auth.json` with no
indication of which codex-session account is affected, and the account
seed would not be updated. Similarly, a raw `codex logout` would revoke
the token server-side but leave the account seed intact — the gate
would still consider the account `Ready` (seed exists), only for the
codex runtime to fail on an invalid token at startup.

By managing login/logout through the gate, every auth operation narrates
which account is being handled and keeps the seed in sync.

## 6. Auth sync after child exit

**Source file:** `pass_through::sync_group_auth_to_seed()`

After every child process (TUI) exit, the wrapper compares the session
group copy (`$CODEX_HOME/auth.json`) with the account seed
(`accounts/<name>/auth.json`). If they differ, the group copy is written
back to the seed via `atomic_write`.

### 6.1 Why this is necessary

During a TUI session, the codex runtime may refresh the OAuth token
(e.g. token_A → token_B). The refreshed token_B is written to
`$CODEX_HOME/auth.json` (the session group copy). Without the sync, the
account seed still holds token_A. If the refresh revoked token_A
server-side, the seed becomes stale — subsequent gate checks or re-added
probes would see an invalid token.

### 6.2 Behavior

| Scenario | Seed | Group copy | Sync action |
|---|---|---|---|
| Token refreshed | token_A | token_B | Overwrites seed with token_B |
| No refresh | token_A | token_A | No-op (bytes match) |
| Group auth missing | token_A | absent | No-op (read fails, early return) |

The sync is best-effort: read or write failures are logged but do not
block the exit path.

## 7. Interaction with the retry loop

The gate runs once at the top of `pass_through::run()`. After the gate
returns `Ready`, the retry loop (`retry::run_with_retry()`) takes over:

1. The retry loop calls `resolver::resolve()` independently on each attempt.
2. On 429 detection, it writes cooldown state and rotates to the next
    account (when `--account auto`).
3. The gate has already validated the initial account. If the retry loop
    exhausts all accounts, it returns `NoEligible`.

The gate does **not** re-run between retry attempts. It is a one-time
pre-launch check. The auth sync runs after **each** child exit
(including retries).

## 8. Error variants

| Variant | Exit code | When |
|---|---|---|
| `NoneResolved` | 64 | Resolver found no account from any source |
| `NoAccounts` | 64 | Registry is empty |
| `NoneSelected` | 64 | Accounts exist but none is selected |
| `AuthMissing { name }` | 75 | Account resolved but seed missing |
| `NonInteractive { action }` | 64 | Interactive prompt needed but no terminal |

## 9. Filesystem layout

```
~/.codex/
  auth.json                      ← native auth (global singleton)

<XDG_STATE_HOME>/codex-session/
  state/
    last-account                 ← LRU pointer (plain text: account name)
  accounts/
    <name>/
      auth.json                  ← account seed (gate checks THIS)
      groups/
        <group-id>/
          auth.json              ← session copy (synced back on exit)
      cooldown.json              ← failover cooldown state
```

## 10. Security properties

- **No implicit defaults.** There is no fallback to a hardcoded `"default"`
  account. Every account must be explicitly registered and authenticated.
- **No hidden auth import.** The deleted `import_if_missing()` function
  previously copied `~/.codex/auth.json` silently into session dirs. Auth
  now flows exclusively through the account seed.
- **Hardened file reads.** `secure_file_read()` checks ownership (must be
  current uid), mode (no group/other bits), no symlinks (`O_NOFOLLOW`), and
  no hardlinks (`nlink == 1`).
- **Directory validation.** `ensure_owned_dir_0700()` creates dirs with
  mode 0700 and refuses symlinked paths.
