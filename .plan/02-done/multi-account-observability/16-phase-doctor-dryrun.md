# Round 6 — Doctor Diagnostics and Dry-Run Account Context

This file is the **prex input** for Round 6. Pass its contents verbatim to
`/prex -ar` after verifying R5 is clean (`just check` exits 0).

---

## Prerequisite

Rounds 1–5 have shipped. The wrapper exposes:

- Stable, persistent `CODEX_HOME` at `<state>/accounts/<account>/groups/<group-id>/` (R1).
- Top-level `account add|list|current|use|remove|quota|cooldown` (R2, R4).
- `--account auto` proactive quota-aware selection (R3).
- Reactive 429 failover, per-account cooldown, retry-with-rotation (R4/R4.5).
- Skills reintegrated with the post-refactor wrapper API (R5).

`just check` exits 0 on the current branch.

## Goal

Enrich the two primary diagnostic interfaces — `doctor` and `--dry-run` — with
per-account health information, fix `hint_for()` for auth/account checks,
surface actionable `next_steps` from WARN conditions, add account context to
`session-meta.json`, and fix the help text to include `cooldown`.

## Background (read these before planning)

- `.plan/multi-account-observability/00-overview.md` — north star for this plan.
- `.plan/multi-account-refactor/00-overview.md` — original R1–R5 overview, glossary.
- `.plan/multi-account-refactor/07-failover-spec.md` — cooldown JSON schema, 429 detection.
- `src/commands/doctor.rs` — 1028 lines. Central target.
  - `DoctorAccountEntry` at lines 64–71: `name`, `has_auth`, `last_used_at_unix`, `current`.
  - `build_report()` at lines 90–272: iterates `registry.list()` at lines 148–167.
  - `hint_for()` at lines 947–971: no case for `auth.native` or `account.*`.
  - `populate_next_steps()` at lines 940–944: only fires for `CheckStatus::Fail`.
  - `check_auth_native()` at lines 775–873: treats missing auth as `ok("auth.native", "no login yet")`.
- `src/domain/child_invocation.rs` — `dry_run_report()` at lines 131–152: shows binary/argv/env but no account.
- `src/commands/pass_through.rs` — dry-run path at lines 40–47, `run_once()` at lines 66–96.
- `src/services/session/meta.rs` — `SessionMeta` at lines 14–19: has profile, group_id, cwd, started_at. No account.
- `src/services/account/cooldown.rs` — `read()` at lines 43–54, `is_active()` at lines 100–102.
- `src/services/account/registry.rs` — `Registry::list()` returns `AccountEntry` with `has_auth`, `account_dir()` returns the path.
- `src/services/account/retry.rs` — `run_with_retry()` at lines 12–136, calls `resolver::resolve()` and `run_once()`.
- `src/ui/help_extras.txt` — line 12 lists account subcommands without `cooldown`.
- `src/ui/mod.rs` — `write_doctor()` renders `DoctorAccountEntry` in text mode.

## Numbered implementation steps

### 1. Extend `DoctorAccountEntry` with cooldown fields

**File:** `src/commands/doctor.rs`, struct at lines 64–71.

Add three fields:

```rust
pub(crate) struct DoctorAccountEntry {
    pub(crate) name: String,
    pub(crate) has_auth: bool,
    pub(crate) last_used_at_unix: Option<u64>,
    pub(crate) current: bool,
    pub(crate) cooldown_active: bool,
    pub(crate) cooldown_reset_at_unix: Option<u64>,
    pub(crate) cooldown_reason: Option<String>,
}
```

In `build_report()` at lines 148–167, when iterating `registry.list()`, read
cooldown state for each account:

```rust
let now_unix = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map(|d| d.as_secs())
    .unwrap_or(0);
// ...inside the .map(|entry| { ... }) closure:
let account_root = registry.account_dir(&entry.id);
let cd = crate::services::account::cooldown::read(&account_root)
    .ok()
    .flatten();
DoctorAccountEntry {
    // ...existing fields...
    cooldown_active: cd.as_ref()
        .is_some_and(|c| crate::services::account::cooldown::is_active(c, now_unix)),
    cooldown_reset_at_unix: cd.as_ref().map(|c| c.reset_at_unix),
    cooldown_reason: cd.map(|c| c.reason),
}
```

Update the text rendering in `src/ui/mod.rs` (`write_doctor`) to include
cooldown status alongside each account entry. Follow the existing format:

```
  work current=true has_auth=true cooldown=active(reset_at=1716500000)
  personal current=false has_auth=true cooldown=none
```

**Files:** `src/commands/doctor.rs`, `src/ui/mod.rs`.

### 2. Add active-account auth check and cooldown summary check

**File:** `src/commands/doctor.rs`.

Add a new function that produces check results for account-level health:

```rust
fn check_account_health(
    accounts: &[DoctorAccountEntry],
    active_name: Option<&str>,
) -> Vec<CheckResult> {
    let mut out = Vec::new();
    if let Some(name) = active_name {
        if let Some(active) = accounts.iter().find(|a| a.name == name) {
            if !active.has_auth {
                out.push(fail(
                    "account.active.auth",
                    format!("active account '{name}' has no auth.json; \
                        codex will fail on first API call"),
                ));
            }
        }
    }
    let cooled: Vec<&str> = accounts.iter()
        .filter(|a| a.cooldown_active)
        .map(|a| a.name.as_str())
        .collect();
    if !cooled.is_empty() {
        out.push(warn(
            "account.cooldowns",
            format!(
                "{} account(s) in cooldown: {}",
                cooled.len(),
                cooled.join(", "),
            ),
        ));
    }
    out
}
```

Insert the call into `build_report()` after the accounts are computed (after
line 167, before the profile checks at line 170). The resolved account name
is available from the `resolved_account` variable:

```rust
let active_name = resolved_account.as_ref().map(|a| a.id.as_str());
checks.extend(check_account_health(&accounts, active_name));
```

**Files:** `src/commands/doctor.rs`.

### 3. Fix `hint_for()` for auth and account checks

**File:** `src/commands/doctor.rs`, function at lines 947–971.

Add new branches before the final fallback:

```rust
} else if name == "auth.native" {
    "run `codex login` then `codex-session account add <name> --from-native`"
} else if name == "account.active.auth" {
    "run `codex login` then `codex-session account add <name> --from-native` to seed auth"
} else if name == "account.cooldowns" {
    "wait for cooldown to expire or run `codex-session account cooldown clear --all`"
} else if name == "session.account" {
    "verify account exists with `codex-session account list` and check `--account` flag"
} else {
    "see check detail"
}
```

**Files:** `src/commands/doctor.rs`.

### 4. Populate `next_steps` from actionable WARN checks

**File:** `src/commands/doctor.rs`, function `populate_next_steps` at lines 940–944.

Currently only FAIL checks populate next_steps. Extend to include specific WARN
check names that have actionable remediation:

```rust
fn populate_next_steps(checks: &[CheckResult], next_steps: &mut Vec<String>) {
    for check in checks.iter().filter(|c| c.status == CheckStatus::Fail) {
        let hint = hint_for(&check.name);
        next_steps.push(format!("{}: {}", check.name, hint));
    }
    for check in checks.iter().filter(|c| c.status == CheckStatus::Warn) {
        if is_actionable_warn(&check.name) {
            let hint = hint_for(&check.name);
            next_steps.push(format!("{}: {}", check.name, hint));
        }
    }
}

fn is_actionable_warn(name: &str) -> bool {
    name == "account.cooldowns"
}
```

Conservative — only `account.cooldowns` is actionable among WARN checks. The
existing `auth.native` WARN (directory mode issue) already self-corrects on
first login. `session.inventory` WARN (stale sessions) could be added later.

**Files:** `src/commands/doctor.rs`.

### 5. Add account context to `--dry-run` report

Two coordinated changes:

**5a. File:** `src/domain/child_invocation.rs`.

Add a context struct and a free function that wraps `dry_run_report()`:

```rust
pub(crate) struct DryRunContext {
    pub(crate) account: String,
    pub(crate) account_source: String,
}

pub(crate) fn dry_run_report_with_context(
    inv: &ChildInvocation,
    context: Option<&DryRunContext>,
) -> String {
    let mut out = String::new();
    if let Some(ctx) = context {
        let _ = writeln!(out, "account: {}", ctx.account);
        let _ = writeln!(out, "account-source: {}", ctx.account_source);
    }
    out.push_str(&inv.dry_run_report());
    out
}
```

This preserves the existing `dry_run_report()` method on `ChildInvocation`
unchanged (backward compatible).

**5b. File:** `src/commands/pass_through.rs`, lines 40–47.

Change the dry-run block to use the enriched report:

```rust
if ctx.global.dry_run {
    let resolved = crate::services::account::resolver::resolve(ctx)?;
    let prepared = prepare_invocation(ctx, argv, &resolved.id)?;
    let dry_ctx = crate::domain::child_invocation::DryRunContext {
        account: resolved.id.to_string(),
        account_source: crate::services::account::resolver::source_label(
            resolved.source,
        )
        .to_owned(),
    };
    ctx.ui.write_dry_run(
        &crate::domain::child_invocation::dry_run_report_with_context(
            &prepared.invocation,
            Some(&dry_ctx),
        ),
    )?;
    tracing::info!(op = "pass-through", status = "ok", outcome = "dry-run");
    return Ok(0);
}
```

**Files:** `src/domain/child_invocation.rs`, `src/commands/pass_through.rs`.

### 6. Add account and account_source to `SessionMeta`

**File:** `src/services/session/meta.rs`, struct at lines 14–19.

Add two fields:

```rust
pub(crate) struct SessionMeta<'a> {
    pub(crate) profile: Option<&'a str>,
    pub(crate) group_id: &'a str,
    pub(crate) cwd: &'a Utf8Path,
    pub(crate) started_at: String,
    pub(crate) account: &'a str,
    pub(crate) account_source: &'a str,
}
```

Update `SessionMeta::new()` to accept the two new parameters.

**Call-site updates:**

The `SessionMeta::new()` call lives in `src/commands/pass_through.rs` inside
`prepare_invocation()`. This function currently takes `account: &AccountId`.
To provide the `account_source`, either:

- (a) Change `prepare_invocation` to accept the full `ResolvedAccount`, or
- (b) Add `account_source: &str` as a separate parameter.

Option (a) is cleaner: `run_once()` already receives `account: &AccountId` and
`retry.rs` already has the `ResolvedAccount` from `resolver::resolve()`. Change
`run_once()` and `prepare_invocation()` to accept `&ResolvedAccount` instead of
`&AccountId`, pull `.id` internally.

Update the call sites:

- `src/commands/pass_through.rs:run()` (dry-run path, line 42): already has
  `resolved` from step 5.
- `src/commands/pass_through.rs:run_once()` (line 66): change signature.
- `src/services/account/retry.rs:run_with_retry()` (line ~77): change the
  `run_once(ctx, argv, &account, ...)` call to pass the full `ResolvedAccount`.

**Files:** `src/services/session/meta.rs`, `src/commands/pass_through.rs`,
`src/services/account/retry.rs`.

### 7. Fix `help_extras.txt` to include `cooldown`

**File:** `src/ui/help_extras.txt`, line 11–12.

Change:

```
  account       Manage codex-session accounts (subcommands: add, list,
                current, use, remove, quota).
```

To:

```
  account       Manage codex-session accounts (subcommands: add, list,
                current, use, remove, quota, cooldown).
```

This changes the `cmd_root_help__root_help.snap` snapshot. Regenerate with
`INSTA_UPDATE=always just test-unit` then `cargo insta review`.

**Files:** `src/ui/help_extras.txt`, snapshot file.

### 8. Tests

New and updated tests:

1. **Update doctor JSON shape test** (`tests/cmd_doctor.rs` or
  `tests/account_doctor.rs`): assert the new `cooldown-active`,
  `cooldown-reset-at-unix`, `cooldown-reason` fields on each account entry.
  In the happy path these are `false` / `null` / `null`.

2. **New test: doctor reports cooldown on accounts.** Create a `TestEnv`, add
  two accounts, write a `cooldown.json` to one account's directory, run
  `doctor --format json`, assert the cooled account shows
  `cooldown-active: true` with valid `reset-at-unix` and `reason`.

3. **New test: doctor warns active account missing auth.** Create an account
  with no auth seed, make it current, run `doctor`, assert the output contains
  `FAIL` + `account.active.auth` + a relevant next_step hint.

4. **New test: doctor cooldown warn appears in next_steps.** Same setup as
  test 2, run text-mode doctor, assert `next_steps` section contains the
  cooldown hint text.

5. **Update dry-run test** (`tests/cmd_dry_run.rs`): add assertions for
  `account:` and `account-source:` lines in the dry-run output.

6. **New test: dry-run shows account from flag.** Run with
  `--account <name> --dry-run exec "hi"`, assert the dry-run output contains
  `account: <name>` and `account-source: flag`.

7. **Update help snapshot** (`tests/snapshots/cmd_root_help__root_help.snap`):
  regenerate due to `cooldown` addition.

8. **Session meta test:** Verify the `session-meta.json` written during a
  pass-through now contains `account` and `account-source` fields. Check if
  an existing test reads `session-meta.json` (likely in
  `tests/account_passthrough.rs` or `tests/cmd_passthrough.rs`) and extend it.
  Otherwise, add a new test.

**Files:** `tests/cmd_doctor.rs` or `tests/account_doctor.rs`,
`tests/cmd_dry_run.rs`, snapshot files.

## Files touched (representative)

| File | What changes |
|---|---|
| `src/commands/doctor.rs` | `DoctorAccountEntry` gains cooldown fields, new `check_account_health()`, `hint_for()` branches, `populate_next_steps()` extended |
| `src/domain/child_invocation.rs` | `DryRunContext` struct, `dry_run_report_with_context()` function |
| `src/commands/pass_through.rs` | Dry-run block uses enriched report, `run_once()` takes `ResolvedAccount` |
| `src/services/session/meta.rs` | `SessionMeta` gains `account` + `account_source` |
| `src/services/account/retry.rs` | Passes `ResolvedAccount` to `run_once()` |
| `src/ui/mod.rs` | Doctor text rendering includes cooldown |
| `src/ui/help_extras.txt` | `cooldown` in account subcommand list |
| `tests/` | ~6–8 new tests, ~3 updated tests, snapshot regeneration |

**Net LOC estimate:** ~350–450 (including ~100 LOC of tests).

## Done criteria

```sh
just check   # must exit 0
```

Manual smoke:

```sh
# Doctor with cooldown
codex-session doctor                              # accounts list shows cooldown=none
codex-session doctor --format json | jq '.accounts[].cooldown-active'

# Doctor with missing auth
codex-session doctor --format json | jq '.checks[] | select(.name == "account.active.auth")'

# Dry-run with account context
codex-session --dry-run exec "hi"                 # shows account: + account-source:
codex-session --account work --dry-run exec "hi"  # account: work, account-source: flag

# Session meta
codex-session exec "echo test"
cat ~/.local/state/codex-session/accounts/default/groups/*/session-meta.json | jq '.account'

# Help text
codex-session --help 2>&1 | grep cooldown
```

## Out of scope (deferred to R7)

- Config status auth enrichment.
- Logging span changes.
- Account remove safety warnings.
- Version account info.
- README/docs updates.
