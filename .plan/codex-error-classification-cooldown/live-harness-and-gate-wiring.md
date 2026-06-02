# Live exec --json Verification Harness & pre-commit Gate Wiring

> Plan: codex-error-classification-cooldown | Round: 1 of 3 | Complexity: L |
> Generated: 2026-06-02 | Repo: /workspaces/codex-session

## Context

`codex-session` wraps the upstream `codex` CLI. A recurring production failure —
heavy `gpt-5.5/high` planning via `codex-session exec --json` getting a `429`
immediately after the model's first tool call, on every account, at ~99%
reported quota — is being fixed by reading codex's **structured `--json` event
stream** to classify errors (later rounds). That work hinges on one uncertain
fact: **does `codex exec --json` actually emit `token_count` events carrying a
non-null `rate_limits` snapshot in exec mode, or is that field null/absent
(openai/codex issue #14728)?**

This round builds the **automated live test** that answers that question on
demand and pins the event/error schema against upstream drift, and wires it into
the project's quality gates **the project's way**: pre-commit hooks are the
Source of Truth, and the `justfile` delegates to them. It also codifies that
principle in `CLAUDE.md` so future contributors (human or agent) keep the gate
plumbing consistent.

Ground truth already established from the installed **codex 0.135.0** binary
(via `strings` on `@openai/codex-linux-x64/.../bin/codex`): `codex exec --json`
emits JSONL events `thread.started`, `turn.started`, `turn.completed`,
`turn.failed`, `item.*`, and `token_count`; `token_count` carries `rate_limits`
= `RateLimitSnapshot { primary, secondary, plan_type, rate_limit_reached_type }`
with each window `RateLimitWindow { used_percent, window_minutes,
resets_in_seconds | resets_at }`. Error discriminants include
`usage_limit_reached`, `usage_limit_exceeded`, `context_window_exceeded`. The
live test verifies whether `rate_limits` is _populated at runtime_ in exec mode.

## Previous Rounds

This is the first round — no prior rounds.

## Scope of This Round

IN scope:

- New live integration test `tests/codex_exec_json_live.rs` (source embedded
  below) that makes one real `codex-session exec --json` call, parses the JSONL
  stream, asserts the load-bearing invariants the later classifier depends on,
  and reports the `#14728` verdict.
- A dedicated pre-commit hook (`cargo-nextest-live`, its own grouping) in
  `.pre-commit-config.yaml`, wired to the **`pre-push`** git stage (per user
  decision — see Decisions), delegating to the existing nextest `live` profile.
- Update the **stale doc comments** in `.pre-commit-config.yaml` and
  `.config/nextest.toml` that currently claim live tests "never run in git
  hooks" / are "manual only" — they must now describe the pre-push live hook.
- Change the `justfile` `test-live` recipe to **delegate to `pre-commit run`**
  instead of calling `cargo nextest` directly (pre-commit is SoT).
- Add a **"pre-commit is the Source of Truth"** principle to `CLAUDE.md`.
- Run the test and record the `RATE_LIMITS_IN_EXEC_MODE` verdict (informs
  Round 2's classifier design).

OUT of scope:

- The `codex_events` reader module, capture changes, and classification
  (Round 2).
- Cooldown duration, transient backoff, unhandled-error UX (Round 3).

## Current State

### Key Files

- `.pre-commit-config.yaml` — SoT for gates. Existing local test hooks to mirror
  in structure (note `stages`):

  ```yaml
  - repo: local
    hooks:
      - id: cargo-nextest-unit
        name: cargo nextest (unit tests)
        entry: cargo nextest run --profile pre-commit --all-features
        language: system
        types: [rust]
        pass_filenames: false
        stages: [pre-commit, pre-push]

      - id: cargo-nextest-integration
        name: cargo nextest (integration tests)
        entry: cargo nextest run --profile pre-push --all-features
        language: system
        types_or: [rust, toml]
        pass_filenames: false
        stages: [pre-push]
  ```

  The header comment block above those hooks currently states the tier-to-hook
  mapping as `live: manual only (`just test-live`, no hook)` — that line, and
  the `Live:` paragraph that says live tests are "Manual invocation only", must
  be updated by this round.

- `.config/nextest.toml` — profiles. The `live` profile already exists and is
  what the new hook will invoke:

  ```toml
  [profile.live]
  default-filter = "binary(/live/)"
  fail-fast = true
  slow-timeout = { period = "30s", terminate-after = 2 }
  status-level = "pass"
  final-status-level = "fail"
  success-output = "immediate"
  failure-output = "immediate-final"
  ```

  The `[profile.pre-push]` filter `kind(test) - binary(/live/)` stays as-is (the
  _integration_ hook must keep excluding live; the new live hook uses the `live`
  profile). Only the **comments** in this file that say live "Never runs in git
  hooks" must be corrected.

- `justfile` — current recipe calls cargo directly; change it to delegate:

  ```text
  test-live:
      CODEX_SESSION_LIVE_TESTS=1 cargo nextest run --profile live --all-features
  ```

  Unit/integration recipes already show the delegation pattern to mirror:
  `pre-commit run --all-files cargo-nextest-unit` and
  `pre-commit run --all-files --hook-stage pre-push cargo-nextest-integration`.

- `tests/account_quota_live.rs` — the existing live-test precedent: the
  `live_tests_enabled()` gate (`CODEX_SESSION_LIVE_TESTS` == `1`/`true`), the
  `assert_cmd::Command::cargo_bin("codex-session")` invocation, and the
  skip-with-`eprintln`-when-disabled convention. Match this style.

- `CLAUDE.md` — already states the justfile is "a thin wrapper over the
  pre-commit hooks ... (the source of truth)" under "Quality gates"; this round
  promotes that to an explicit, named principle.

### Existing Patterns

- Live test binaries are selected by the `/live/` name convention
  (`binary(/live/)`); a file named `*_live.rs` is auto-included in the `live`
  profile and auto-excluded from `pre-push`. No nextest filter edit needed for a
  new `*_live.rs` file.
- `Result`/`thiserror`, no `unwrap()` in production (tests may
  `#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]` as the
  existing live test does).
- Quality gates run via `just` → `pre-commit`, never raw cargo.

## Implementation Steps

### Step 1: Create `tests/codex_exec_json_live.rs`

Create the file with this content (a single `#[test]` so only one billable exec
call is made):

```rust
//! Live verification of the `codex exec --json` event-stream schema.
//!
//! Answers the question the error-classification work depends on: does
//! `codex exec --json` emit `token_count` events carrying a non-null
//! `rate_limits` snapshot in exec mode, or is it null/absent (openai/codex
//! issue #14728)? Also pins the broader event/error schema (`turn.failed`,
//! `RateLimitWindow` field shapes) so upstream drift surfaces here.
//!
//! Makes one real, billable `codex` API call via `codex-session exec` with a
//! harmless no-op prompt. Does NOT hard-fail when the run is itself
//! rate-limited — that case is captured and reported.
//!
//! Tier: Live. Selected by nextest `[profile.live]` via `binary(/live/)`.
//! Invocation: `CODEX_SESSION_LIVE_TESTS=1 just test-live` (delegates to the
//! `cargo-nextest-live` pre-commit hook), or directly for an ad-hoc focused run:
//! `CODEX_SESSION_LIVE_TESTS=1 cargo nextest run --profile live -E 'binary(codex_exec_json_live)'`.
//!
//! Prerequisites: an account with valid OAuth credentials + quota; network.
//!
//! Verdict lines to read in the output:
//!   [codex-exec-json] event types observed: {...}
//!   [codex-exec-json] RATE_LIMITS_IN_EXEC_MODE: populated | null | absent-from-token_count | no-token_count-event
//!   [codex-exec-json] RUN_RATE_LIMITED: true | false

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(missing_docs)]

use std::collections::BTreeSet;

const NOOP_PROMPT: &str =
    "Reply with exactly the two characters: OK. Do not run any commands, do not \
     use any tools, do not read or write any files.";

fn live_tests_enabled() -> bool {
    std::env::var("CODEX_SESSION_LIVE_TESTS")
        .ok()
        .is_some_and(|v| v == "1" || v == "true")
}

/// Recursively search a JSON tree for the first value under `key`.
fn find_key<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(found) = map.get(key) {
                return Some(found);
            }
            map.values().find_map(|v| find_key(v, key))
        }
        serde_json::Value::Array(arr) => arr.iter().find_map(|v| find_key(v, key)),
        _ => None,
    }
}

fn looks_rate_limited(haystack: &str) -> bool {
    let h = haystack.to_ascii_lowercase();
    h.contains("usage_limit_reached")
        || h.contains("usage_limit_exceeded")
        || h.contains("hit your usage limit")
        || h.contains("rate_limit")
        || h.contains("rate limit")
        || h.contains("too many requests")
        || h.contains(" 429")
        || h.contains("\"429\"")
}

fn assert_window_shape(label: &str, window: &serde_json::Value) {
    assert!(window.is_object(), "rate_limits.{label} is not an object: {window}");
    let used = window.get("used_percent").and_then(serde_json::Value::as_f64);
    assert!(
        used.is_some(),
        "rate_limits.{label}.used_percent missing or not a number: {window}. \
         RateLimitWindow shape changed upstream — update codex_events.rs + docs.",
    );
    let has_reset = window.get("resets_in_seconds").is_some_and(|v| v.is_number())
        || window.get("resets_at").is_some_and(|v| v.is_number());
    assert!(
        has_reset,
        "rate_limits.{label} carries neither numeric `resets_in_seconds` nor \
         `resets_at`: {window}. Reset-timing field changed upstream.",
    );
}

#[test]
fn live_exec_json_event_schema() {
    if !live_tests_enabled() {
        eprintln!(
            "skipped: set CODEX_SESSION_LIVE_TESTS=1 to run live tests \
             (or use `just test-live`)"
        );
        return;
    }

    let output = assert_cmd::Command::cargo_bin("codex-session")
        .unwrap()
        .args(["exec", "--json", NOOP_PROMPT])
        .write_stdin("")
        .output()
        .expect("failed to execute codex-session");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let events: Vec<serde_json::Value> = stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .collect();

    let run_rate_limited = looks_rate_limited(&stdout) || looks_rate_limited(&stderr);

    assert!(
        !events.is_empty(),
        "no JSONL events parsed from `codex-session exec --json`.\n\
         Was --json honored? Is an account authed with quota?\n\
         exit: {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status.code(),
    );

    let types: BTreeSet<&str> = events
        .iter()
        .filter_map(|e| e.get("type").and_then(serde_json::Value::as_str))
        .collect();
    eprintln!("[codex-exec-json] event types observed: {types:?}");
    eprintln!("[codex-exec-json] RUN_RATE_LIMITED: {run_rate_limited}");

    let token_count = events
        .iter()
        .rev()
        .find(|e| e.get("type").and_then(serde_json::Value::as_str) == Some("token_count"));
    let rate_limits = token_count.and_then(|e| find_key(e, "rate_limits"));

    let verdict = match rate_limits {
        Some(v) if v.is_null() => "null",
        Some(_) => "populated",
        None if token_count.is_some() => "absent-from-token_count",
        None => "no-token_count-event",
    };
    eprintln!("[codex-exec-json] RATE_LIMITS_IN_EXEC_MODE: {verdict}");
    if let Some(rl) = rate_limits {
        eprintln!("[codex-exec-json] rate_limits snapshot: {rl}");
    }

    assert!(
        token_count.is_some() || run_rate_limited,
        "event stream had neither a `token_count` event nor a recognized \
         rate/usage-limit signal — schema may have drifted.\n\
         event types: {types:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
    );

    if let Some(rl) = rate_limits {
        if !rl.is_null() {
            if let Some(primary) = rl.get("primary").filter(|v| !v.is_null()) {
                assert_window_shape("primary", primary);
            }
            if let Some(secondary) = rl.get("secondary").filter(|v| !v.is_null()) {
                assert_window_shape("secondary", secondary);
            }
            assert!(
                rl.get("primary").is_some_and(|v| !v.is_null())
                    || rl.get("secondary").is_some_and(|v| !v.is_null()),
                "rate_limits populated but has neither primary nor secondary window: {rl}",
            );
        }
    }
}
```

Confirm it compiles and skips safely without the env var (no API call):
`cargo test --test codex_exec_json_live -- --nocapture` should print the skip
line and pass.

### Step 2: Add the `cargo-nextest-live` pre-commit hook (own group, pre-push)

In `.pre-commit-config.yaml`, add a dedicated local hook (its own grouping). The
entry must export `CODEX_SESSION_LIVE_TESTS=1` (the test self-gates) and use the
`live` nextest profile:

```yaml
- repo: local
  hooks:
    - id: cargo-nextest-live
      name: cargo nextest (live API tests)
      entry: env CODEX_SESSION_LIVE_TESTS=1 cargo nextest run --profile live --all-features
      language: system
      types: [rust]
      pass_filenames: false
      stages: [pre-push]
```

Then update the **stale doc comments** in the same file: the tier-to-hook
mapping line `live: manual only (`just test-live`, no hook)` becomes
`live: pre-push (cargo-nextest-live hook) +`just test-live``, and the `Live:`
paragraph's "Manual invocation only" / "Excluded from pre-push" wording must be
corrected to state that live now runs on `pre-push` via the dedicated hook
(requires creds + network).

### Step 3: Correct the `.config/nextest.toml` comments

Update the comments in `[profile.pre-push]` and `[profile.live]` that say live
tests "Never runs in git hooks" / belong to a "manual" tier — they now run on
`pre-push` via the `cargo-nextest-live` hook. Do **not** change the
`[profile.pre-push]` filter (`kind(test) - binary(/live/)` stays — the
integration hook still excludes live; the live hook uses `[profile.live]`).

### Step 4: Make `justfile test-live` delegate to pre-commit

Replace the direct cargo call so the recipe delegates to the SoT hook (mirror the
unit/integration recipes), and update the recipe's comment accordingly:

```text
# Live API tests — hit real endpoints. Requires real OAuth credentials +
# network. Delegates to the SoT pre-commit hook (pre-push stage).
test-live:
    pre-commit run --all-files --hook-stage pre-push cargo-nextest-live
```

### Step 5: Add the "pre-commit is the Source of Truth" principle to `CLAUDE.md`

In the "Quality gates" section of `/workspaces/codex-session/CLAUDE.md`, promote
the existing parenthetical into an explicit, named principle, e.g.:

> **Principle — pre-commit hooks are the Source of Truth for quality gates.** The
> `justfile` gate recipes MUST delegate to `pre-commit run …` rather than invoke
> `cargo` directly. A new test/lint tier is defined as a pre-commit hook first
> (in `.pre-commit-config.yaml`), then exposed via a `just` recipe that calls
> that hook. Raw `cargo` in the justfile is reserved for inner-loop, non-gate
> recipes (`build`, `run`, `fmt`, `fix`, `watch`, `clean`).

Keep it concise and consistent with the existing section's voice.

### Step 6: Run the live test and record the verdict

Run the gate the SoT way and capture the reported verdict:

```bash
just test-live
```

Record the `RATE_LIMITS_IN_EXEC_MODE` value (`populated` / `null` /
`absent-from-token_count` / `no-token_count-event`) and `RUN_RATE_LIMITED` in the
commit message / notes — Round 2's classifier design depends on whether snapshot
windows are available in exec mode. If creds/quota are unavailable in this
environment, note that the verdict is pending and proceed (the test still
compiles and is wired; the verdict can be captured later).

### Final Step: Update the queue

Record completion in the queue — status lives in YAML; nothing moves on disk:

1. In this plan's `_QUEUE.yaml`, set this round's
   (`item: live-harness-and-gate-wiring`) `status` to `done`.

## Acceptance Criteria

- [ ] `tests/codex_exec_json_live.rs` exists, compiles, and skips (prints the
      skip line, passes) when `CODEX_SESSION_LIVE_TESTS` is unset — no API call.
- [ ] `.pre-commit-config.yaml` has a `cargo-nextest-live` hook (own group) at
      `stages: [pre-push]` delegating to the `live` nextest profile with the env
      var set; the stale "live = manual only / no hook" comments are corrected.
- [ ] `.config/nextest.toml` comments no longer claim live "never runs in git
      hooks"; the `pre-push` filter is unchanged.
- [ ] `justfile` `test-live` delegates to
      `pre-commit run --all-files --hook-stage pre-push cargo-nextest-live`
      (no direct cargo call).
- [ ] `CLAUDE.md` states the "pre-commit is the Source of Truth" principle.
- [ ] `just test-live` runs the hook; the `RATE_LIMITS_IN_EXEC_MODE` verdict is
      recorded (or noted as pending if creds/quota unavailable).
- [ ] This plan's `_QUEUE.yaml` shows round `live-harness-and-gate-wiring` as
      `done`.

## Next Round

Round 2 (`event-reader-and-classification`) builds the structured event reader
(`codex_events.rs`), the tail-preserving capture, and the classification layer in
`failover.rs`, using this round's verdict to decide how much to rely on
`token_count.rate_limits` vs `turn.failed` + `retry_after`. It may extend this
round's live test to assert the concrete `turn.failed` nesting once observed.
