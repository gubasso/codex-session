# Round 01: Core selection engine — auto by default, exec-only picking

> Plan: account-auto-default-selection | Round: 01 of 03 | Complexity: L Generated: 2026-05-29 |
> Repo: /workspaces/codex-session

## Context

`codex-session` wraps OpenAI's `codex` CLI and multiplexes several accounts, each under its own
`CODEX_HOME=<state>/accounts/<account>/groups/<group>/`. Today an account is only auto-selected when
the user explicitly passes `--account auto`; with no flag the resolver falls through a stale chain
(`registry.current()` LRU → `config.account.pinned` → `NoneResolved` error).

This round makes the wrapper behave like stock `codex exec`: **no account argument ⇒ auto-select an
enabled account.** `--account <name>` (or `CODEX_SESSION_ACCOUNT=<name>`) pins; `--account auto` /
`=auto` stay as explicit aliases of the default. Auto **rotates across accounts on a mid-run 401/429
with no cycling** (each account tried at most once), emits a concise stderr warning per switch, and
fails with a multi-line per-account explanatory error.

Critically, the quota-aware scoring **selector must fire ONLY on the positive exec path**. The
resolver is split into "interpret inputs" (pure) vs "pick an account" (scoring, side effects), so
read-only commands never trigger a pick. Because the old `resolve()` is removed and every caller
must change to keep the tree compiling, this is one atomic round. No back-compat shims.

## Previous Rounds

This is the first round — no prior rounds.

## Scope of This Round

IN scope:

- Split `resolver.rs` into `intent` / `resolve_for_exec(exclude)` / `resolve_for_display`.
- `selector::pick(ctx, exclude)`: add exclude-set filter; **remove the `set_current` side effect**;
  extract shared `is_usable` / `skip_reason` helpers.
- `retry.rs`: replace `run_with_retry` with `run_auto` (auto-only, no-cycling, stderr warnings) plus
  `single_attempt` (pinned/interactive/resume); commit `set_current` on exec success.
- `gate.rs`: `ensure` returns a `GateOutcome { Resolved | AutoDeferred }`; readiness checks only —
  no pick.
- `pass_through.rs`: rewire `run` and `run_resume` to pick exactly once.
- Errors: remove `NoneResolved`; add `AutoExhausted { report }`; update exit-code map + messages.
- Migrate every display caller off the removed `resolve()` to `resolve_for_display`:
  `version`, `config_status`, `doctor`, `account/list`, `account/current`, `context::get_or_resolve`
  (feeds `config-recipe compose`).
- Unit + integration tests for the above.

OUT of scope (later rounds):

- Removing the `account use` command, `config.account.pinned`, `CODEX_SESSION_ACCOUNT_PINNED`,
  `--account` help-text rewording, root-help snapshot (Round 02).
- Documentation sweep across repo / `~/DocsNNotes` / `~/.dotfiles` (Round 03).

Note: `config.account.pinned` becomes unread after this round (the resolver no longer consults it).
Leave the field defined — Round 02 removes it. Do not touch `src/commands/account/use_.rs` in this
round (it calls `registry.set_current`, not `resolve()`).

## Current State

### Key Files

- `/workspaces/codex-session/src/services/account/resolver.rs` — the chain to replace. Current:

  ```rust
  pub(crate) enum AccountResolutionSource {
      Flag, Env, Auto, Lru, ConfigPinned, Interactive, ThreadIndex,
  }
  pub(crate) struct ResolvedAccount { pub(crate) id: AccountId, pub(crate) source: AccountResolutionSource }

  pub(crate) fn resolve(ctx) -> Result<ResolvedAccount, AppError> {
      resolve_from_inputs(ctx, std::env::var("CODEX_SESSION_ACCOUNT").ok())
  }
  // flag Named→Flag; flag Auto→selector::pick→Auto; env name→Env / env "auto"→pick→Auto;
  // registry.current()→Lru; config.account.pinned→ConfigPinned; else NoneResolved error.
  ```

- `/workspaces/codex-session/src/services/account/selector.rs` — `pick(ctx)` (no exclude param).
  Reads `let lru = registry.current()?;` for the recency penalty (`is_lru → -30` in
  `score_for_display`), and at the end calls `registry.set_current(&picked.id)?;` — the side effect
  to remove. Eligibility filter (skip `!has_auth`, `cooldown_active`, `token_expired`) is inline in
  `pick`.

- `/workspaces/codex-session/src/services/account/retry.rs` — `run_with_retry` (the rotation-opt-in
  block computes `rotation_opted_in = flag_auto || (env_auto && account.is_none())`; warns + does
  `single_attempt` when `max_retries>0 && !rotation_opted_in`). Loop `for attempt in 0..=max_retries`
  calls `resolver::resolve(ctx)`, `run_once(ctx, argv, &resolved, &signal_session, capture, None)`
  with `let capture = max_retries > 0;`, scans via `failover::pick_priority(failover::scan(&stderr),
  failover::scan(&stdout))`, handles `AuthFailure` (token_refresh → `retry_same`) and `RateLimit`
  (cooldown), `continue`s to rotate, exhaustion → `Err(NoEligible)`.

- `/workspaces/codex-session/src/services/account/gate.rs` — `ensure(ctx)` calls `assess` →
  `resolver::resolve()` (line ~59), catches `NoneResolved`, has stale-pointer `NotFound` handling
  (an LRU artifact) and interactive prompting that calls `registry.set_current`.

- `/workspaces/codex-session/src/commands/pass_through.rs` — `run`:

  ```rust
  let gated = crate::services::account::gate::ensure(ctx)?;
  if let Some(intent) = detect_resume(argv) { return run_resume(ctx, argv, &intent, &gated); }
  if ctx.global.dry_run { /* uses gated */ }
  crate::services::account::retry::run_with_retry(ctx, argv)   // RE-RESOLVES, ignores `gated`
  ```

  `run_once(... capture ...)` doc: "When `capture` is true, the child's streams are tee'd to the
  parent's real stdio AND independently captured" — so failover scanning does not regress streaming.
  `run_resume` (~lines 528-597) pins via thread-index (`ThreadIndex`) and only falls back to
  `run_with_retry` on a thread-index miss.

- `/workspaces/codex-session/src/services/account/error.rs` — `AccountError` enum incl.
  `NoneResolved` (msg ~43-45), `NoneSelected`, `NoAccounts`, `NotFound`, `NoEligible`, `AuthMissing`;
  `kind()` and `path()` impls.

- `/workspaces/codex-session/src/error.rs` — exit-code map (`NoneResolved`/`NoneSelected`/
  `NoAccounts`/`InvalidName`→64; `NotFound`→78; `NoEligible`/`LoginFailed`/`AuthMissing`→75) and
  `error_detail` / user-facing message arms.

- Display callers of `resolver::resolve()` to migrate: `src/commands/version.rs` (~37),
  `src/commands/config_status.rs` (~76), `src/commands/doctor.rs` (~110), `src/commands/account/list.rs`
  (~9), `src/commands/account/current.rs` (~8), `src/context.rs` `get_or_resolve` (~69, used by
  `src/commands/config_recipe_compose.rs:37`). Each builds an `AccountCurrentView { name, source }`
  (see `src/commands/account/mod.rs`).

### Existing Patterns

- User-facing warnings: `ctx.ui.write_warning(&str)` → stderr (respects `--quiet`/`--silent`); see
  `src/ui/mod.rs`. Diagnostics use `tracing::info!/warn!/debug!` with `op = "…"` keys.
- `AccountId` is validated (`src/services/account/id.rs`); a literal account named `auto` is
  impossible because `AccountSelector::from_str` reserves `"auto"` (`src/cli/account.rs`).
- `Registry` (`src/services/account/registry.rs`): `current()` reads `state/last-account`;
  `set_current(&id)` atomically writes it; `list()` enumerates accounts with `has_auth`,
  `last_used_at`.
- Clippy is strict (`just lint`); add `#[allow(clippy::result_large_err)]` consistent with the
  existing modules.

## Implementation Steps

### Step 1: Resolver — split intent vs pick vs display

In `/workspaces/codex-session/src/services/account/resolver.rs`:

- Trim `AccountResolutionSource` to `{ Flag, Env, Auto, Interactive, ThreadIndex }` (drop `Lru`,
  `ConfigPinned`); update `source_label` accordingly.
- Add:

  ```rust
  pub(crate) enum AccountIntent {
      Pinned { id: AccountId, source: AccountResolutionSource }, // Flag or Env
      Auto,
  }
  pub(crate) fn intent(ctx: &AppContext) -> Result<AccountIntent, AppError>;
  fn intent_from_inputs(ctx: &AppContext, env: Option<String>) -> Result<AccountIntent, AppError>;
  ```

  Order in `intent_from_inputs`: flag `Named(id)` → `Pinned{Flag}`; flag `Auto` → `Auto`; else env:
  `"auto"` → `Auto`, non-empty other → parse (`InvalidName` on failure) → `Pinned{Env}`; else (no
  flag, empty/absent env) → **`Auto`** (the new default). No registry/selector/IO; total except
  `InvalidName`.

- Add the exec resolver:

  ```rust
  pub(crate) fn resolve_for_exec(ctx: &AppContext, exclude: &HashSet<AccountId>)
      -> Result<ResolvedAccount, AppError>;
  ```

  `Pinned{id,source}` → return directly (ignore `exclude`); `Auto` → `selector::pick(ctx, exclude)`
  → `ResolvedAccount{ id, source: Auto }`.

- Add the display resolver:

  ```rust
  pub(crate) enum DisplayAccount {
      Pinned { id: AccountId, source: AccountResolutionSource },
      Auto { last_selected: Option<AccountId> }, // reads registry.current()
  }
  pub(crate) fn resolve_for_display(ctx: &AppContext) -> Result<DisplayAccount, AppError>;
  ```

  `intent(ctx)`; `Pinned` → as-is; `Auto` → `Registry::from_config(&ctx.config).current()?` →
  `Auto { last_selected }`. No pick, no quota, no `set_current`.

- Delete `resolve` / `resolve_from_inputs`. Rewrite inline tests: drop `lru_wins_over_config`,
  `config_pinned_is_used`, `no_sources_returns_none_resolved`; adapt flag/env/auto tests to
  `intent_from_inputs` and `resolve_for_exec`; drop the `pinned` param from the `test_ctx` helper.

### Step 2: Selector — exclude set, no side effects, shared helpers

In `/workspaces/codex-session/src/services/account/selector.rs`:

- Change signature to `pub(crate) fn pick(ctx: &AppContext, exclude: &HashSet<AccountId>)
  -> Result<AccountId, AccountError>`.
- In the candidate filter, add `if exclude.contains(&entry.id) { tracing::debug!(account=%entry.id,
  reason="already-tried"); continue; }`.
- **Delete `registry.set_current(&picked.id)?;`** — `pick` becomes side-effect-free (quota cache
  writes aside). The LRU recency penalty still reads `registry.current()` (the last _committed_
  selection from a prior invocation); within-invocation no-repeat is handled by `exclude`.
- Extract from the filter two reusable helpers (used by gate and the exhaustion error):

  ```rust
  pub(crate) fn is_usable(ctx: &AppContext, reg: &Registry, entry: &AccountEntry) -> bool;
  pub(crate) fn skip_reason(ctx: &AppContext, reg: &Registry, entry: &AccountEntry) -> Option<String>;
  // None = usable; Some("no auth" | "cooldown until <RFC3339>" | "token expired" |
  //                      "below quota threshold (5h <p>% / weekly <p>%)")
  ```

  `is_usable` = `skip_reason(...).is_none()` over the auth/cooldown/token gates (quota threshold is
  evaluated during scoring, so `is_usable` covers auth + cooldown + token; `skip_reason` may
  additionally report the quota-threshold case when a `Known` quota is below threshold).

### Step 3: Errors — drop `NoneResolved`, add `AutoExhausted`

In `/workspaces/codex-session/src/services/account/error.rs`:

- Remove the `NoneResolved` variant and its `kind()`/`path()` arms.
- Add:

  ```rust
  #[error("auto-selection exhausted; no account could complete the request")]
  AutoExhausted { report: Vec<AccountOutcomeLine> },

  pub(crate) struct AccountOutcomeLine { pub(crate) id: AccountId, pub(crate) outcome: String }
  ```

  `kind()` → `"account-auto-exhausted"`; `path()` → `None`.
- Update the `NoneSelected` message to drop the `account use` reference, e.g.: "no account selected;
  pass `--account <name>` to pin one, or run `codex-session login`."

In `/workspaces/codex-session/src/error.rs`:

- Remove `NoneResolved` from the exit-code match and from `error_detail` / user-facing arms.
- Map `AutoExhausted` to exit **75** (alongside `NoEligible`/`AuthMissing`).
- Render `AutoExhausted` as a multi-line detail, one line per account
  (`• <id>   <outcome>`), ending with a hint:
  "Run `codex-session account health` for details, or clear cooldowns with
  `codex-session account cooldown clear --all`."
- Update the `NoneSelected` why-line to match the new message.

### Step 4: Gate — readiness only, return `GateOutcome`

In `/workspaces/codex-session/src/services/account/gate.rs`:

- Rework `assess` to operate on `intent` (not a resolved pick), returning:

  ```rust
  enum AccountState {
      ReadyPinned(ResolvedAccount), ReadyAuto,
      AuthMissing { account: AccountId }, PinnedNotFound { account: AccountId },
      NoneSelected { accounts: Vec<AccountEntry> }, NoAccounts,
  }
  ```

  Flow: `list()` empty → `NoAccounts`. `Pinned{id,source}` → `expect_account_dir(id)`: `NotFound` ⇒
  `PinnedNotFound`; seed missing ⇒ `AuthMissing`; else ⇒ `ReadyPinned`. `Auto` → if ≥1 entry with
  `selector::is_usable` ⇒ `ReadyAuto`, else (accounts exist, none usable) ⇒ `NoneSelected`. (The old
  stale-pointer branch becomes `PinnedNotFound`.)

- `ensure` returns `enum GateOutcome { Resolved(ResolvedAccount), AutoDeferred }`:
  `ReadyPinned(r)` → narrate, `Resolved(r)`; `ReadyAuto` → narrate "auto-selection enabled (N
  candidates)", `AutoDeferred`; interactive recovery for `NoAccounts`/`NoneSelected`/`AuthMissing`
  returns `Resolved(ResolvedAccount{ Interactive })`; non-interactive errors: `PinnedNotFound` →
  `NotFound` (78), `NoneSelected` → `NoneSelected` (64), `AuthMissing` → `AuthMissing` (75),
  `NoAccounts` → exit 64.

### Step 5: Retry — `run_auto` (no-cycling) + `single_attempt`

In `/workspaces/codex-session/src/services/account/retry.rs`, replace `run_with_retry` (and the
rotation-opt-in block) with:

```rust
pub(crate) fn run_auto(ctx: &AppContext, argv: &[OsString]) -> Result<i32, AppError>;
fn single_attempt(ctx, argv, resolved: &ResolvedAccount) -> Result<i32, AppError>; // capture=false, no failover
```

`run_auto` loop (capture ALWAYS on — only entered for auto; tee preserves streaming):

```text
registry = Registry::from_config(ctx.config); signal = SignalSession::install()?;
eligible = registry.list()?.iter().filter(|e| selector::is_usable(ctx,&registry,e)).count();
cap = if max_retries==0 { eligible } else { (max_retries as usize + 1).min(eligible) };
tried: HashSet<AccountId> = {}; report: Vec<AccountOutcomeLine> = []; force_same: Option<ResolvedAccount> = None;
loop {
    if tried.len() >= cap && force_same.is_none() { break; }
    resolved = force_same.take() OR match resolve_for_exec(ctx,&tried) {
        Ok(r)=>r, Err(NoEligible)=>break, Err(e)=>return Err(e) };
    first_use = tried.insert(resolved.id.clone());
    (exit, out, err) = run_once(ctx, argv, &resolved, &signal, /*capture*/true, None)?;
    match failover::pick_priority(failover::scan(&err), failover::scan(&out)) {
        None => { let _ = registry.set_current(&resolved.id); return Ok(exit); } // commit on success
        Some(m) => match m.kind {
            AuthFailure => { if first_use && try_refresh(ctx,&resolved.id) { force_same=Some(resolved.clone()); continue; }
                write_cooldown(&registry,&resolved.id,"401",&m); report.push(line(&resolved.id,"401 auth failed")); }
            RateLimit   => { write_cooldown(&registry,&resolved.id,"429",&m); report.push(line(&resolved.id,"429 rate limit")); }
        }
    }
    ctx.ui.write_warning(&format!("account '{}' hit {}; rotating to next account…", resolved.id, kind_label))?;
}
Err(AccountError::AutoExhausted { report: build_report(ctx,&registry,report,&tried) }.into())
```

- `tried.insert` records the id BEFORE the run, so a refreshed account that 401s again has
  `first_use==false` on its re-run and cannot re-refresh — it cools down and rotates. No cycling.
- `build_report` merges three sources so every registered account appears with a reason: ran-and-
  failed (from `report`), selector-skipped (`selector::skip_reason`), and never-reached ("not
  attempted (rotation cap reached)").
- Keep `now_unix`, cooldown writing, and token-refresh helpers; reuse existing `failover` scanning.

### Step 6: pass_through — pick exactly once

In `/workspaces/codex-session/src/commands/pass_through.rs`:

```rust
let outcome = crate::services::account::gate::ensure(ctx)?;
if let Some(intent) = detect_resume(argv) {
    return run_resume(ctx, argv, &intent, outcome.resolved_or_none()); // Option<&ResolvedAccount>
}
if ctx.global.dry_run {
    let resolved = match &outcome {
        GateOutcome::Resolved(r) => r.clone(),
        GateOutcome::AutoDeferred => resolve_for_exec(ctx, &HashSet::new())?, // one pick to display
    };
    /* existing dry-run report using `resolved` */ return Ok(0);
}
match outcome {
    GateOutcome::Resolved(r)   => retry::single_attempt(ctx, argv, &r),
    GateOutcome::AutoDeferred  => retry::run_auto(ctx, argv),
}
```

- Change `run_resume`'s `gated` parameter to `Option<&ResolvedAccount>`. Thread-index HIT path is
  unchanged (pins `ThreadIndex`). On thread-index MISS: if `Some(pinned)` → `single_attempt`; if
  `None` (auto) → `run_auto`.

### Step 7: Migrate display/inspection callers off `resolve()`

Switch each to `resolver::resolve_for_display` and render the auto-unset case as
`name = "(auto — none selected yet)"`, `source = "auto"` (or `account: None` where the view is
optional):

- `/workspaces/codex-session/src/commands/account/current.rs` — map `DisplayAccount` to the view.
- `/workspaces/codex-session/src/commands/account/list.rs` — drop the `NoneResolved` match arm;
  compute `active_name` from the display value (`Pinned.id`, or `Auto.last_selected`, or `None`).
- `/workspaces/codex-session/src/commands/version.rs` — `resolve_for_display(ctx).ok()`; auto-unset
  → `account: None`, `account_source: Some("auto")`.
- `/workspaces/codex-session/src/commands/config_status.rs` — drop the `NoneResolved` arm; auto-unset
  → `resolved_account = None` (the existing missing-session-dir branch still applies).
- `/workspaces/codex-session/src/commands/doctor.rs` — auto-unset is a PASSING check showing
  "(auto — none selected yet)", not a `fail`. Only a broken pinned name / registry IO fails.
- `/workspaces/codex-session/src/context.rs` `get_or_resolve` — use `resolve_for_display`; auto-unset
  ⇒ return `AccountError::NoneSelected` (a never-run auto pool has no session dir for
  `config-recipe compose` to inspect; `config_recipe_compose.rs:37` already propagates `AppError`).

### Step 8: Tests

- Unit: rewrite `resolver.rs` tests around `intent_from_inputs` / `resolve_for_exec` /
  `resolve_for_display` (incl. "no flag, no env ⇒ Auto"). Add a `selector::pick` test asserting an
  excluded id is never chosen. Add `run_auto` tests: 429 on A rotates to B and succeeds; A→B→C then
  exhaust never re-picks A; token-refresh retries the same account once then rotates; `AutoExhausted`
  report lists every account with a reason.
- Integration (`/workspaces/codex-session/tests/`): update `account_resolution.rs` (drop LRU/config
  ordering tests; add "no-arg ⇒ auto" and "pinned ⇒ no rotation"); update `account_failover_pinned.rs`
  for the pinned-never-rotates path; fix `session_resume_routing.rs` thread-index-miss test to seed a
  usable account (a miss now auto-selects, not LRU).
- Run `just lint` and `just test` until green.

### Final Step: Update plan index

Update the plan's `README.md` (same directory as this round file):

1. In the `## Execution Order` table, find the row for round 01.
2. Change `Status` from `todo` to `done`.
3. Change `Completed` from `--` to today's date (`YYYY-MM-DD`).

## Acceptance Criteria

- [ ] `codex-session exec …` with no `--account` auto-selects an enabled account (source `auto`).
- [ ] `--account <name>` / `CODEX_SESSION_ACCOUNT=<name>` pins and never rotates; a missing pinned
      name errors `NotFound` (exit 78).
- [ ] `--account auto` / `=auto` still work as explicit aliases of the default.
- [ ] On a mid-run 429/401, auto rotates to the next account with a stderr warning; each account is
      tried at most once (no A→B→C→A); exhaustion yields a multi-line `AutoExhausted` error (exit 75)
      listing every account with a reason.
- [ ] The scoring selector fires ONLY on the exec path: `doctor`, `version`, `config status`,
      `config-recipe compose`, `account list`, `account current` perform NO pick — verified via `-vv`
      (no `account.select` / `set_current` / quota fetch on those commands).
- [ ] `selector::pick` is side-effect-free (no `set_current`); `set_current` is written only on a
      successful exec attempt.
- [ ] `NoneResolved` is removed; the tree compiles; `just lint` and `just test` pass.
- [ ] Plan `README.md` execution order table shows round 01 as `done` with today's date.

## Next Round

Round 02 removes the now-orphaned `account use` command and the dead `config.account.pinned` /
`CODEX_SESSION_ACCOUNT_PINNED` config surface (unread after this round), reweords the `--account`
help text, and refreshes the root-help snapshot.
