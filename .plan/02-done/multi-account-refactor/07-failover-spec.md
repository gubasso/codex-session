# 07 — Failover Spec

Reactive 429 handling via caam-borrowed regex detector + per-account cooldown file + retry-with-rotation. Implements R4's `src/services/account/{failover,retry,cooldown}.rs` and `src/commands/account/cooldown.rs`.

## Trigger surface

A 429 is signaled by codex (or upstream OpenAI) writing any of the following patterns to stdout or stderr. Patterns are **verbatim from caam** (`internal/ratelimit/detector.go`, `DefaultPatterns()[ProviderCodex]`):

```
(?i)rate.?limit
(?i)quota.?exceeded
\b429\b
(?i)too.?many.?requests
(?i)exceeded.*rate
(?i)slow.?down
```

Where:

- `(?i)` — case-insensitive.
- `\b` — word boundary (so `429` matches but `9429` does not).
- `.?` — one optional character of any class. So `(?i)rate.?limit` matches `rate-limit`, `rate limit`, AND `rate_limit` (underscore). This is caam's actual behavior; the JSON key `rate_limit:` is therefore a positive match, not a negative.
- The six patterns are evaluated as a `regex::RegexSet`; `scan()` returns the **first** matching pattern's index (the lowest index in `PATTERN_NAMES`).

> **Spec-drift note.** An earlier draft of this spec quoted a single invented alternation `(?i)\b(429|rate[- ]limit|too many requests|quota exceeded|slow down)\b` and called it "verbatim from caam". R4's `/prex` plan-review caught the mismatch against the actual caam source; R4.5 updates this spec to match what the code now ships. See [ADR D9 references](04-decisions.md) and `.plan/multi-account-refactor/14-phase-r4-hardening.md` step 13 for the migration record.

### Pattern test matrix (R4.5 unit tests)

| Input line | Match? | Lowest-index pattern that fires |
|---|---|---|
| `HTTP 429 Too Many Requests` | ✅ | `\b429\b` (index 2) |
| `Error: rate limit exceeded` | ✅ | `(?i)rate.?limit` (index 0) |
| `Error: rate-limit exceeded` | ✅ | `(?i)rate.?limit` (index 0) |
| `rate_limit: { ... }` (JSON key) | ✅ | `(?i)rate.?limit` (index 0) — `.` is a wildcard |
| `quota exceeded; retry after 300 s` | ✅ | `(?i)quota.?exceeded` (index 1) |
| `Server says slow down` | ✅ | `(?i)slow.?down` (index 5) |
| `Server says slow-down` | ✅ | `(?i)slow.?down` (index 5) |
| `Error: exceeded rate quota` | ✅ | `(?i)exceeded.*rate` (index 4) |
| `Got 9429 errors today` | ✗ | — (no word boundary) |
| `four hundred twenty nine` | ✗ | — (no numeric `429`, no rate-limit phrase) |
| `nominal capacity reached` | ✗ | — (Codex list does NOT include `capacity`; Claude's list does — this lockdown row guards against accidentally adopting the Claude pattern set) |

`Match { line_no, snippet, pattern_index }` carries the matched index; structured logs enrich it via `PATTERN_NAMES[pattern_index]` (one of `rate-limit`, `quota-exceeded`, `429`, `too-many-requests`, `exceeded-rate`, `slow-down`).

## Detection placement

`services/account/failover.rs` implements a passive observer:

```rust
pub(crate) fn scan(buf: &[u8]) -> Option<Match>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Match {
    pub line_no: usize,
    pub snippet: String,        // the matched line, truncated to 256 chars
    pub pattern_index: usize,   // index into PATTERN_NAMES
}

pub(crate) const PATTERN_NAMES: [&str; 6] = [
    "rate-limit", "quota-exceeded", "429",
    "too-many-requests", "exceeded-rate", "slow-down",
];
```

**Tee strategy:** real-time forwarding **plus** post-wait scan. (Updated by R4.5; see [ADR D9](04-decisions.md).)

Rationale:

- Existing `Spawner` was fork/exec/wait without piping. R4 piped stdio + `wait_with_output()`, which broke interactive Codex by buffering all output until the child exited.
- R4.5 keeps stdio piped (so the wrapper can scan it) but interposes two reader threads in `Spawner::spawn_and_wait_output`. Each thread copies 8 KiB chunks off the child's pipe and writes them to BOTH the parent's real stdout/stderr (live) AND a per-stream `Vec<u8>` (capture). After `child.wait()`, the threads join cleanly (the kernel keeps pipes readable past the writer's death), and `failover::scan` runs on the captured buffers.
- For the `codex exec` workload (one-shot, non-TUI) the post-wait scan is sufficient; the live forwarding only matters for the interactive `codex-session` TUI invocation. Both work after R4.5.
- Streaming detection (kill the child on first match, mid-flight) is **still** out of scope — it would let the wrapper save tokens on a doomed run but adds significant complexity around `kill_process` ordering. Defer until a real use case emerges.

**Implementation:** the tee logic lives in `src/ui/raw_passthrough.rs::tee_to_stdio` (not `adapters/spawner.rs`) so the print-ownership lint whitelist stays narrow. The capture buffer is capped at `failover::MAX_CAPTURE_BYTES = 1 << 20` (1 MiB); past that limit, live forwarding continues unaffected but the capture half stops appending. The detector splits the capture on `b'\n'` so truncation at a non-line boundary is safe (the next line just doesn't get matched).

**TTY caveat.** Because stdout/stderr are pipes from the child's perspective, `isatty(fd) == 0`. Codex's TUI/color machinery will detect non-interactive output and degrade. The trade-off is unavoidable as long as the wrapper needs to see the child's bytes for failover; the established workaround is `CODEX_FORCE_TTY=1` (or the equivalent env codex exposes) when full interactive mode is required. R4.5 documents this in `--help` and in `99-execution-plan.md`'s manual smoke section.

## Cooldown file schema

```
<registry-root>/<account>/cooldown.json
```

where `<registry-root>` is `config.account.registry_dir` (or `CODEX_SESSION_ACCOUNT_REGISTRY_DIR` env override) when set, else `<state-root>/accounts`. The path is computed via `Registry::account_dir(&account).join("cooldown.json")` — R4.5 routes through the registry so the custom-`registry_dir` config knob keeps working (this was R4 finding I2).

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
- **R4.5 update:** rotation is only meaningful when the user explicitly opts into auto-selection (`--account auto` or `CODEX_SESSION_ACCOUNT=auto`). For every other resolution path — `--account <name>`, `CODEX_SESSION_ACCOUNT=<name>`, `state/last-account` LRU, `config.account.{pinned,default}`, or the built-in default — the retry harness short-circuits before the loop and runs a **single** `pass_through::run_once` attempt: it does NOT loop, does NOT write a cooldown file, and emits a structured warning (`op=account.pinned_retry_noop`) plus a stderr line containing `"ignored"` and `"--account auto"` so the user knows their `--max-retries` was suppressed. Rationale: a single-account resolver would just re-execute the same account each iteration; that's write-amplification with no failover benefit (R4 finding I6, widened in R4.5 review-loop round 3).

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
    "cooled-down": true,
    "reset-at-unix": 1716393600,
    "reset-in-seconds": 192,
    "reason": "429 detected: ...",
    "last-429-at-unix": 1716386400
  },
  ...
]
```

`account cooldown clear` is destructive (deletes the file). Without `--account` or `--all`, it errors with `EX_USAGE` (64).

## Logging

Structured events emitted by the failover path:

| op | level | fields | when |
|---|---|---|---|
| `failover.match` | info | `account`, `snippet`, `line_no`, `pattern_index`, `pattern`, `attempt` | A 429 pattern matched the child output |
| `account.switch` | warn | `from`, `reason="429"`, `attempt` | Retry switched to a new account |
| `cooldown.write` | info | `account`, `reset_at_unix`, `reason` | Cooldown file written |
| `cooldown.clear` | info | `account` or `"all"` | `cooldown clear` invoked |
| `retry.exhausted` | error | `max_retries`, `last_account` | All retries used, returning NoEligible |

## Interaction with other features

| Feature | Interaction |
|---|---|
| `--max-retries 0` (default) | Detector runs but never retries. Cooldown file is NOT written (no point — single shot). |
| `--max-retries > 0` without `--account auto` (named flag, env, LRU, config, default) | **R4.5:** harness short-circuits to a single attempt; emits `op=account.pinned_retry_noop` and a stderr warning containing `"ignored"` and `"--account auto"`. No cooldown file is written. Use `--account auto` (or `CODEX_SESSION_ACCOUNT=auto`) for actual rotation. |
| `--max-retries > 0` + `--account auto` | Full reactive failover. |
| `--account auto` + no eligible accounts | `AccountError::NoEligible` (exit code 75, `EX_TEMPFAIL`). |
| Cooldown reset_at expired | Selector treats account as eligible again automatically (no cleanup needed — file is harmless once expired; `cooldown clear --all` can sweep). |
| Multiple parallel codex-session processes | Cooldown writes are atomic (per-account file); no contention. Two processes hitting 429 on the same account both write; last-writer-wins is acceptable (timestamps are close enough). |

## Test strategy (R4 + R4.5)

`tests/account_failover_*.rs`, `tests/account_cooldown_*.rs`, `tests/cmd_signal_*.rs`:

| Test | Setup |
|---|---|
| Detector matches each caam pattern (8 positive cases incl. `rate_limit:`) | table-driven, no spawn needed |
| Detector rejects `9429`, spelled-out, `capacity` | negative table (R4.5 swaps `rate_limit:` from negative → positive and adds `capacity` lockdown) |
| Retry with `--max-retries 2 --account auto` finds different account | `tests/fixtures/fake-429.sh` always emits 429; two accounts registered; verify second invocation uses different account; third returns NoEligible (exit 75) |
| **R4.5:** pinned `--account <name> --max-retries 2` short-circuits | one attempt only, `"ignored"` substring in stderr, no cooldown file written, child exit code surfaces |
| Cooldown JSON round-trips | write + read symmetrically; `Decode` vs `Encode` error variants differ |
| **R4.5:** `cooldown show` JSON is kebab-case | parse output, assert `cooled-down` / `reset-at-unix` / `last-429-at-unix` keys |
| `cooldown show --json` schema | parse output, assert shape |
| **R4.5:** `cooldown clear --all` rejects `--account X` combo | exit 64, stderr contains `"conflicts with"` |
| `cooldown clear --account <name>` removes one | populate 3, clear one, verify only that one removed |
| **R4.5:** custom `CODEX_SESSION_ACCOUNT_REGISTRY_DIR` round-trip | cooldown writes appear under the override path; default state-root has none |
| **R4.5:** SIGINT mid-stream preserves output | `tests/fixtures/slow-streamer.sh` emits a line every 50 ms; send SIGINT after 250 ms; assert pre-kill lines visible in captured stderr, exit code `128+SIGINT` |
| Race: two processes write cooldown.json simultaneously | spawn 2 fakes; both write; verify file is well-formed JSON (last-writer-wins acceptable per ADR D6) |

## Open questions deferred to follow-up

- **Should `Retry-After` parsing be more lenient?** Currently only honored if surfaced in child output as `Retry-After: <seconds>` or `retry-after: ...`. The child rarely surfaces HTTP headers — typically just the response body. Default `now + 300 s` will dominate in practice.
- **Per-account telemetry on 429 rate** (e.g., "this account got 429'd N times this week") — useful for the future TUI dashboard. Out of scope for R4.
