# R9: Explicit Auth Gate — No Hidden Fallbacks

## Context

The current TUI launch path (`codex-session` with no subcommand) silently falls
through a resolution chain that ends with a hardcoded `"default"` account name —
even if no account is registered. Additionally, `auth::import_if_missing()`
silently copies `~/.codex/auth.json` into the session directory, creating a
hidden auth path. The `--from-current` flag on `account add` is another implicit
shortcut. All of these violate the principle that **every authentication must be
explicit**.

This round introduces an **auth gate** that runs before every TUI launch.
It assesses the account state, and either proceeds (happy path) or guides the
user through interactive setup / selection / re-authentication — or, in
non-interactive mode, fails with an actionable error message.

### Design principles

- **No implicit defaults** — no `"default"` account, no hidden fallback, no
  `--from-current`, no silent import from `~/.codex/auth.json`.
- **Two modes** — interactive (terminal prompts via `inquire` crate) and
  non-interactive (CLI flags; hard error with actionable message if preconditions
  unmet).
- **DRY** — the four scenarios share reusable action primitives.
- **Stale pointers are graceful** — a resolved account whose directory is missing
  warns and falls into "none selected" flow rather than hard-erroring.

### User scenarios

| Scenario | State | Interactive behavior | Non-interactive behavior |
|---|---|---|---|
| A. Happy path | Account selected + has auth | Launch TUI | Launch TUI |
| B. Auth missing | Account selected, no/expired auth | Menu: re-auth / switch / add | Error: "run `account refresh`" |
| C. None selected | Accounts exist, none pinned/LRU'd | Menu: select account / add | Error: "run `account use`" |
| D. First time | No accounts at all | Guided first-account setup | Error: "run `account add`" |

---

## Phase 1 — Removals

Strip out every implicit/hidden auth path so the resolver can return "nothing
resolved" instead of always succeeding.

### 1a. Remove `AccountId::default()` impl

**File:** `src/services/account/id.rs`

- Delete the `impl Default for AccountId` block (lines 28–32) that returns
  `"default"`.
- Delete the `default_is_default` test.
- Fix all compilation errors from sites that relied on `AccountId::default()`.

### 1b. Remove `AccountResolutionSource::Default` and `Fallback`

**File:** `src/services/account/resolver.rs`

- Remove the `Default` and `Fallback` variants from `AccountResolutionSource`.
- Remove their arms from `source_label()`.
- Remove the `config.account.default` resolution arm (lines 103–109).
- Remove the fallback arm (lines 111–116).
- When no source matches, return `Err(AccountError::NoneResolved)` (new variant,
  see Phase 2).
- Add a new `Interactive` variant for accounts selected by the gate's prompts.
  Add `"interactive"` to `source_label()`.
- Update tests: remove `default` parameter from `test_ctx()`, add a
  `no_sources_returns_none_resolved` test.

### 1c. Remove `config.account.default` field

**File:** `src/config/mod.rs`

- Remove the `default` field from `AccountConfig` (line 69).
- Remove `FileAccountConfig.default` and its apply logic (~line 410).
- Remove the `CODEX_SESSION_ACCOUNT_DEFAULT` env-var handler (~line 491).
- Remove/update the `invalid_account_default_in_file_layer_errors` test.

**File:** `src/ui/help_extras.txt` — remove `CODEX_SESSION_ACCOUNT_DEFAULT`
documentation.

### 1d. Remove `--from-current` flag

**File:** `src/cli/account.rs`
- Remove `from_current: bool` from `AccountAddArgs` (lines 36–38).

**File:** `src/commands/account/add.rs`
- Remove the entire `if args.from_current { ... }` branch (lines 12–24).
- The interactive `codex login` flow becomes the only add path.

**File:** `src/services/account/registry.rs`
- Remove `from_current: bool` and `native_home: &Utf8Path` parameters from
  `Registry::add()`. The method now only creates the directory structure
  (`accounts/<name>/` + `accounts/<name>/groups/`). Auth seeding happens in the
  command handler after `codex login`.
- Update all callers and tests.
- Delete the `add_from_current_fails_without_native_auth` test.

### 1e. Delete `auth::import_if_missing()`

**File:** `src/services/auth.rs`
- Delete the `import_if_missing()` function entirely. The gate guarantees auth is
  present before launch; `materialize_account_auth_seed()` already copies
  account-root `auth.json` to the session group dir.

**File:** `src/commands/pass_through.rs`
- Remove the call to `auth::import_if_missing()` in `run_child()` (line 246).

---

## Phase 2 — New Error Variants

**File:** `src/services/account/error.rs`

Add to `AccountError`:

```rust
#[error("no account could be resolved; run `codex-session account add <name>` or pass --account <name>")]
NoneResolved,

#[error("account `{name}` has no valid authentication; run `codex-session account refresh {name}`")]
AuthMissing { name: AccountId },

#[error("no accounts registered; run `codex-session account add <name>`")]
NoAccounts,

#[error("accounts exist but none is selected; run `codex-session account use <name>` or pass --account <name>")]
NoneSelected,
```

Update `kind()` and `path()` match arms for each new variant.

---

## Phase 3 — The Gate Module (core)

**New file:** `src/services/account/gate.rs`

**New dependency:** `inquire` crate (for `Select`, `Text` prompt widgets).

### 3a. `AccountState` enum

Captures the four possible pre-launch states:

```rust
pub(crate) enum AccountState {
    /// A: Account selected and has auth — ready to launch.
    Ready(ResolvedAccount),
    /// B: Account selected but auth is missing/expired.
    AuthMissing {
        account: AccountId,
        source: AccountResolutionSource,
    },
    /// C: Accounts exist but none is selected.
    NoneSelected {
        accounts: Vec<registry::AccountEntry>,
    },
    /// D: No accounts registered at all.
    NoAccounts,
}
```

### 3b. `assess()` — Pure state assessment (no side effects)

```rust
pub(crate) fn assess(ctx: &AppContext) -> Result<AccountState, AppError>
```

Logic:

1. `registry.list()?` → if empty → `AccountState::NoAccounts`.
2. `resolver::resolve(ctx)`:
    - `Err(AccountError::NoneResolved)` → `NoneSelected { accounts }`.
    - `Ok(resolved)` → check `registry.expect_account_dir(&resolved.id)`:
      - `Err(AccountError::NotFound { .. })` → **warn** about stale LRU/config
        pointer → fall into `NoneSelected { accounts }`.
      - Ok → find account in list, check `has_auth`:
        - `true` → `Ready(resolved)`.
        - `false` → `AuthMissing { account: resolved.id, source: resolved.source }`.
    - Other errors → propagate.

This function is testable in isolation (no I/O side effects).

### 3c. `ensure()` — Gatekeeper entry point

```rust
pub(crate) fn ensure(ctx: &AppContext) -> Result<ResolvedAccount, AppError>
```

1. Call `assess(ctx)`.
2. `Ready` → return immediately.
3. Check `std::io::stdin().is_terminal()`:
    - **Non-interactive**: return the matching `AccountError` variant
      (`NoAccounts`, `NoneSelected`, `AuthMissing`) with actionable error message.
    - **Interactive**: call `interactive_resolve()`.

### 3d. `interactive_resolve()` — Interactive remediation flows

Uses `inquire::Select` for menus and `inquire::Text` for name input.

**DRY action primitives** (private functions):

| Primitive | What it does | Reused by |
|---|---|---|
| `do_add_account(ctx) -> Result<AccountId>` | Prompt name via `Text`, create account dir, run `codex logout` + `codex login`, copy native auth to seed, `set_current`. Reuses `account::spawn_child` and `copy_native_auth_to_seed`. | D, C("add new"), B("add new") |
| `do_refresh_auth(ctx, &AccountId) -> Result<()>` | Run `codex logout` + `codex login`, copy native auth, delete group auths. Mirrors `account refresh` command logic. | B("re-auth") |
| `prompt_select_account(accounts) -> Result<AccountId>` | `Select` menu showing name + auth status per entry, plus "Add a new account" option. | C, B("switch") |

**Flow dispatch by state:**

**D (`NoAccounts`)**:
```
No accounts registered. Let's set up your first account.
[Text prompt: account name]
[codex logout + codex login]
[seed auth, set_current]
→ Ready
```

**C (`NoneSelected`)**:
```
[Select menu]:
  work (authenticated)
  personal (authenticated)
  test (no auth!)
  ➕ Add a new account

[If selected account has auth → set_current → Ready]
[If selected account has no auth → fall into B flow]
[If "Add" → do_add_account → Ready]
```

**B (`AuthMissing`)**:
```
Account 'work' has no valid authentication.
[Select menu]:
  Re-authenticate 'work'
  Switch to a different account
  Add a new account

[Re-auth → do_refresh_auth → Ready]
[Switch → prompt_select_account (loop if selected also lacks auth)]
[Add → do_add_account → Ready]
```

After any action succeeds:
- `registry.set_current(&account_id)`
- Return `ResolvedAccount { id, source: AccountResolutionSource::Interactive }`

### 3e. Register module

**File:** `src/services/account/mod.rs` — add `pub(crate) mod gate;`.

---

## Phase 4 — Integration

### 4a. Wire gate into pass-through

**File:** `src/commands/pass_through.rs`

In `run()`, call `gate::ensure(ctx)?` before both the dry-run path and the retry
path:

```rust
pub(crate) fn run(ctx: &AppContext, argv: &[OsString]) -> Result<i32, AppError> {
    tracing::info!(op = "pass-through", status = "start", argc = argv.len());
    let gated = crate::services::account::gate::ensure(ctx)?;

    if ctx.global.dry_run {
        let prepared = prepare_invocation(ctx, argv, &gated)?;
        // ... dry_run rendering using gated ...
        return Ok(0);
    }
    crate::services::account::retry::run_with_retry(ctx, argv)
}
```

The retry loop continues to call `resolver::resolve()` internally for rotation
(auto mode). The gate has already validated the initial account.

### 4b. Update all `resolver::resolve()` callers

Every caller must handle the new `NoneResolved` error gracefully:

| Caller | File | Strategy |
|---|---|---|
| `version::run` | `src/commands/version.rs:37` | Already uses `.ok()` — no change needed. |
| `LazySession::get_or_resolve` | `src/context.rs:65` | Propagate error — callers handle it. |
| `config_status::run` | `src/commands/config_status.rs:76` | Catch `NoneResolved`, show `"(no account)"`. |
| `list::run` | `src/commands/account/list.rs:13` | Catch `NoneResolved`, list accounts with no active marker. |
| `current::run` | `src/commands/account/current.rs:8` | Propagate — clear "no account selected" error. |
| `quota::run` | `src/commands/account/quota.rs:32` | Catch `NoneResolved`, error with actionable hint. |
| `doctor::run` | `src/commands/doctor.rs:110` | Already wraps in `Result` — handle gracefully. |
| `retry::run_with_retry` | `src/services/account/retry.rs:68,157` | Gate runs first; `NoneResolved` here means no accounts to rotate to — valid error to propagate. |

### 4c. Update retry module comments

**File:** `src/services/account/retry.rs`

Remove references to `config.account.{default}` and `fallback` in the comment at
line 17.

---

## Phase 5 — Update doctor hints

**File:** `src/commands/doctor.rs`

- Remove `account.default` from hint text.
- Update hints that reference `--from-current`.
- Account-health checks should report `NoneResolved` gracefully.

---

## Phase 6 — Add `inquire` dependency

**File:** `Cargo.toml`

Add `inquire = "0.7"` (or latest stable) to `[dependencies]`.

---

## Verification

1. `just lint` — clippy-strict + fmt + print-ownership pass.
2. `just test` — all unit + integration tests pass.
3. **Manual test matrix** (interactive terminal):
    - Fresh state (no accounts) → gate prompts to add first account.
    - One account, has auth → TUI launches normally.
    - One account, no auth → gate prompts to re-authenticate.
    - Multiple accounts, none selected → gate shows selection menu.
    - Non-interactive (`echo | codex-session`) with no account → clear error.
    - `--account work` with existing auth → bypasses gate, launches.
    - `--account work` with missing auth → gate reports error (non-interactive).
    - `--dry-run` → shows gated account in dry-run report.
4. `just check` — full pre-push gate.

---

## Files changed (summary)

| File | Action |
|---|---|
| `src/services/account/gate.rs` | **NEW** — gate module with `assess()`, `ensure()`, interactive flows |
| `src/services/account/mod.rs` | Add `pub(crate) mod gate;` |
| `src/services/account/resolver.rs` | Remove Default/Fallback; add NoneResolved error path + Interactive variant |
| `src/services/account/id.rs` | Remove `Default` impl |
| `src/services/account/error.rs` | Add 4 new error variants |
| `src/services/account/registry.rs` | Remove `from_current`/`native_home` params from `add()` |
| `src/config/mod.rs` | Remove `account.default` field + all plumbing |
| `src/cli/account.rs` | Remove `--from-current` from `AccountAddArgs` |
| `src/commands/account/add.rs` | Remove `from_current` branch |
| `src/commands/pass_through.rs` | Wire `gate::ensure()`; remove `import_if_missing` call |
| `src/services/auth.rs` | Delete `import_if_missing()` |
| `src/commands/config_status.rs` | Handle `NoneResolved` gracefully |
| `src/commands/account/list.rs` | Handle `NoneResolved` gracefully |
| `src/commands/account/current.rs` | Propagate clear error |
| `src/commands/account/quota.rs` | Handle `NoneResolved` gracefully |
| `src/commands/doctor.rs` | Handle `NoneResolved`; update hints |
| `src/services/account/retry.rs` | Update comments |
| `src/context.rs` | Propagate `NoneResolved` from `LazySession` |
| `src/ui/help_extras.txt` | Remove `ACCOUNT_DEFAULT` doc |
| `Cargo.toml` | Add `inquire` dependency |

---
