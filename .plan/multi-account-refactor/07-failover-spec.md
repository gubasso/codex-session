# 07 — Failover Spec

Reactive 429 handling via caam-borrowed regex detector + per-account cooldown file + retry-with-rotation. Implements R4's `src/services/account/{failover,retry,cooldown}.rs` and `src/commands/account/cooldown.rs`.

## Trigger surface

A 429 is signaled by codex (or upstream OpenAI) writing one of the following patterns to stdout or stderr. Patterns are **verbatim from caam** (`internal/ratelimit/detector.go`):

```
(?i)\b(429|rate[- ]limit|too many requests|quota exceeded|slow down)\b
```

Where:

- `(?i)` — case-insensitive.
- `\b` — word boundary (so `429` matches but `9429` does not).
- `rate[- ]limit` — matches both `rate-limit` and `rate limit` (with hyphen or space). Does NOT match `rate_limit` (underscore) — that's the JSON key, not a phrase.

### Pattern test matrix (R4 unit tests)

| Input line | Match? |
|---|---|
| `HTTP 429 Too Many Requests` | ✅ (`429`, `too many requests`) |
| `Error: rate limit exceeded` | ✅ (`rate limit`) |
| `Error: rate-limit exceeded` | ✅ (`rate-limit`) |
| `quota exceeded; retry after 300 s` | ✅ (`quota exceeded`) |
| `Server says slow down` | ✅ (`slow down`) |
| `Got 9429 errors today` | ✗ (no word boundary) |
| `rate_limit: { ... }` (JSON key) | ✗ (underscore, not hyphen/space) |
| `four hundred twenty nine` (spelled out) | ✗ |

## Detection placement

`services/account/failover.rs` implements a passive observer:

```rust
pub struct Detector { ... }

impl Detector {
    /// Scan a captured stdout+stderr buffer for any caam pattern.
    /// Returns the first match (line index + matched substring) or None.
    pub fn scan(buf: &[u8]) -> Option<Match>;
}

pub struct Match {
    pub line_no: usize,
    pub snippet: String,        // the matched line, truncated to 256 chars
}
```

**Tee strategy:** post-wait scan, not mid-flight streaming.

Rationale (per ADR / R4 difficulty rationale in `99-execution-plan.md`):

- Existing `Spawner` does fork/exec/wait without piping.
- Streaming detection would require a `PipingSpawner` refactor (touch the whole spawn path).
- For our `codex exec` workload (one-shot, non-TUI), post-wait detection is sufficient — we only need to know "did this fail with 429?" to decide on retry.
- If a long-lived TUI use case emerges later, augment with a `PipingSpawner`; until then, KISS.

**Implementation:** the existing spawn already inherits stdout/stderr by default. R4 changes `pass_through::run_once` to capture them (via `Stdio::piped()` + `wait_with_output()`), forward the captured bytes to the parent's stdout/stderr unmodified after the child exits, then run `Detector::scan` on the combined buffer. Net effect on user experience: the child's output still appears, just slightly after the child exits (acceptable latency for the `codex exec` workload).

## Cooldown file schema

```
<state-root>/accounts/<account>/cooldown.json
```

```json
{
  "reset_at_unix": 1716393600,
  "reason": "429 detected: \"HTTP 429 Too Many Requests\"",
  "last_429_at_unix": 1716386400,
  "snippet_truncated": "HTTP 429 Too Many Requests"
}
```

- **`reset_at_unix`** — derived from `Retry-After` header if surfaced in the child output; else `now + 300 s` (5 minutes; the actual 5-hour window starts later, but we just need to skip *this* selector pass).
- **`reason`** — human-readable; never used for control flow.
- **`last_429_at_unix`** — for telemetry and `cooldown show`.
- **`snippet_truncated`** — for debugging; the matched line, truncated.

**Atomic write** via `src/adapters/fs.rs::atomic_write`.

The selector (R3) reads `cooldown.json` per account. If `reset_at_unix > now`, the account is **disqualified** entirely from scoring.

## Retry-with-rotation harness

```
commands::pass_through::run(ctx, argv)
  ↓
services::account::retry::run(ctx, argv)
  ↓
  for attempt in 0..=max_retries {
    account_id = resolver::resolve(ctx, --account/env/...)
    result = pass_through::run_once(ctx, argv, account_id)
    if attempt < max_retries && Detector::scan(captured_output).is_some() {
      cooldown::write(account_id, reset_at)
      tracing::warn!(op = "account.switch",
                      from = %account_id,
                      reason = "429",
                      attempt = attempt + 1);
      continue;
    }
    return result;
  }
  return Err(AccountError::NoEligible)  // max retries exhausted
```

- `max_retries` defaults to **0** (off). User opts in via `--max-retries N` global flag.
- Each retry call re-runs the resolver (so `--account auto` re-runs the selector, which now excludes the cooled-down account).
- If the resolver is pinned (`--account <name>`), retry rotation is a no-op — the same account is picked again. Document this in `help_extras.txt`. (Future enhancement: `--account auto` should be required for retry to be meaningful. R4 emits a warning if `--max-retries > 0` is set with an explicit pinned account.)

## CLI surface

```sh
codex-session account cooldown                          # alias of `cooldown show`
codex-session account cooldown show                     # human-readable table
codex-session account cooldown show --json              # array of {account, reset_at, reason, ...}
codex-session account cooldown show --account work      # one account only
codex-session account cooldown clear --account work     # remove that account's cooldown.json
codex-session account cooldown clear --all              # remove all cooldown.json files
```

Default `cooldown show` output (human-readable):

```
ACCOUNT     STATUS       RESETS         REASON
work        cooled-down  in 3m 12s      "HTTP 429 Too Many Requests"
personal    eligible     —              —
default     eligible     —              —
```

`--json` schema:

```json
[
  {
    "account": "work",
    "cooled_down": true,
    "reset_at_unix": 1716393600,
    "reset_in_seconds": 192,
    "reason": "429 detected: ...",
    "last_429_at_unix": 1716386400
  },
  ...
]
```

`account cooldown clear` is destructive (deletes the file). Without `--account` or `--all`, it errors with `EX_USAGE` (64).

## Logging

Structured events emitted by the failover path:

| op | level | fields | when |
|---|---|---|---|
| `failover.match` | info | `account`, `snippet`, `line_no`, `attempt` | A 429 pattern matched the child output |
| `account.switch` | warn | `from`, `to`, `reason="429"`, `attempt` | Retry switched to a new account |
| `cooldown.write` | info | `account`, `reset_at_unix`, `reason` | Cooldown file written |
| `cooldown.clear` | info | `account` or `"all"` | `cooldown clear` invoked |
| `retry.exhausted` | error | `max_retries`, `last_account` | All retries used, returning NoEligible |

## Interaction with other features

| Feature | Interaction |
|---|---|
| `--max-retries 0` (default) | Detector runs but never retries. Cooldown file is NOT written (no point — single shot). |
| `--max-retries > 0` + `--account <pinned>` | Warns: "retries with pinned account are no-ops; use --account auto". |
| `--max-retries > 0` + `--account auto` | Full reactive failover. |
| `--account auto` + no eligible accounts | `AccountError::NoEligible` (exit code 75, `EX_TEMPFAIL`). |
| Cooldown reset_at expired | Selector treats account as eligible again automatically (no cleanup needed — file is harmless once expired; `cooldown clear --all` can sweep). |
| Multiple parallel codex-session processes | Cooldown writes are atomic (per-account file); no contention. Two processes hitting 429 on the same account both write; last-writer-wins is acceptable (timestamps are close enough). |

## Test strategy (R4)

`tests/account_failover_*.rs`:

| Test | Setup |
|---|---|
| Detector matches each caam pattern (8 cases) | table-driven, no spawn needed |
| Detector rejects `9429`, `rate_limit:`, etc. | negative table |
| Retry with `--max-retries 2` finds different account | `tests/fixtures/fake-429.sh` always emits 429; two accounts registered; verify second invocation uses different account; third returns NoEligible |
| Retry with `--account <pinned>` skips rotation | pinned account; verify retry loop runs but selects same account → eventually NoEligible |
| Cooldown JSON round-trips | write + read symmetrically |
| `cooldown show` human format | snapshot test |
| `cooldown show --json` schema | parse output, assert shape |
| `cooldown clear --all` removes all files | populate 3 cooldowns, clear, list dirs |
| `cooldown clear --account <name>` removes one | populate 3, clear one, verify only that one removed |
| Race: two processes write cooldown.json simultaneously | spawn 2 fakes; both write; verify file is well-formed JSON |

## Open questions deferred to follow-up

- **Should `Retry-After` parsing be more lenient?** Currently only honored if surfaced in child output as `Retry-After: <seconds>` or `retry-after: ...`. The child rarely surfaces HTTP headers — typically just the response body. Default `now + 300 s` will dominate in practice.
- **Per-account telemetry on 429 rate** (e.g., "this account got 429'd N times this week") — useful for the future TUI dashboard. Out of scope for R4.
