# Round 04 — `account health` probe vs. quota auth-source authority

> Round 4 (deferred follow-up, added after Round 03) | Topic: close the
> stale-seed/live-group false-negative by making the health probe and quota agree
> on the authoritative auth file | Status: todo

## Context

Deferred from Round 03 (prex stage-5 review-loop, 2026-06-01). Round 03 explicitly
scoped this out ("Out of scope: Re-architecting auth-file authority beyond what
Step 0 needs"); this round is that deferred work. Rounds 01–03 are complete; this
round was appended so the auth-authority work stays with the plan that surfaced it.

## Goal

Eliminate the false `token=invalid` / `token=unknown` that `account health` can
report for a **healthy** account when the account's **seed** `auth.json` is stale
but a per-group `auth.json` holds a newer (live) token.

## The defect

`account health` always probes the account **seed**:
`gate::probe_token` → `registry.group_auth_seed_path(account)` → `heartbeat_probe`
(`src/services/account/gate.rs`). Meanwhile `quota::resolve_auth_path` prefers a
**current/newest group** `auth.json` (`src/services/account/quota.rs:492-494`),
falling back to the seed. Per
[docs/upstream-codex.md](/workspaces/codex-session/docs/upstream-codex.md) §F12,
each per-group `CODEX_HOME` owns its own `auth.json` that codex rotates **in
place**, so after normal pass-through use a group file can hold a newer token than
the seed.

When the seed is stale but a group file is live **and quota does not rotate during
the run**, `reconcile_probe_after_quota_rotation` (which only reacts to an observed
quota rotation) does nothing, and health reports a false negative for a healthy
account.

### Scope / accuracy notes (verified 2026-06-01)

- **Pre-existing**, not introduced by Round 03 — the probe already read the seed at
  HEAD. Round 03's up-front `refresh_health_seed_if_needed` does **not** worsen it:
  `refresh_token` only writes back on success, so a failed refresh of a stale/dead
  seed token corrupts and orphans nothing.
- **Distinct** from the documented no-group concurrent last-writer residual at
  `src/commands/account/health.rs` (Round 03 Step 0c comment). That residual is the
  rare mid-flight-expiry write race; this is the steady-state stale-seed-vs-live-group
  read mismatch.

## Approach (pick one in plan-review)

1. **Probe the source quota resolves (minimal).** In `build_entry`, when
   `active_auth` resolves to a group file `!= seed`, probe via
   `gate::probe_token_with_auth(active_auth)` so health reflects the token quota
   will actually use. Smallest change; keeps the seed as the persisted source of
   truth but reads the live one for the health verdict.
2. **Sync live group → seed before probing.** Establish the seed as the single
   source of truth by copying the newest live group `auth.json` back to the seed
   before the probe. Heavier; interacts with the Step 0 single-flight refresh and
   the rotation-reconcile logic.
3. **Full seed-vs-group auth-authority redesign.** Define one authoritative
   resolution order used by probe, quota, and refresh alike. Largest; resolves the
   ambiguity for all consumers, not just health.

> Resolve which auth file is authoritative against
> [docs/upstream-codex.md](/workspaces/codex-session/docs/upstream-codex.md) and the
> registry code before implementing — do not guess. Whichever is chosen, the Step 0
> refresh coordination (single-flight + reconcile) must stay consistent with it.

## Acceptance Criteria

1. A stale-seed / live-group account reports `token=ok` (no false negative) when the
   group token is valid and quota does not rotate during the run.
2. No regression to Round 03's refresh-race guarantees (both directions): expired
   token still refreshes exactly once and persists the rotation; live token triggers
   zero refreshes.
3. Regression test covering stale-seed + live-group + (no rotation / rotation)
   permutations, using the canonical wiremock token-endpoint + persisted-bytes
   pattern (`tests/account_health_refresh_race.rs`).
4. `just check` green; no new warnings; no global `#[allow]` shims.

## Out of scope

- Pass-through group-copy semantics beyond what the chosen approach requires.
- Changing quota's resolution order unless approach (3) is selected.
