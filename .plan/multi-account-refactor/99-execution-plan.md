# 99 — Execution Plan

## Phase grouping (one round = one `/prex` invocation)

Each round is a **single coherent goal** sized to one prex round (Stage 1 + Stage 3 each ≤600 s Codex budget). Per-round sizing target: 4–8 numbered plan steps, 1–5 files touched per step, ~300–700 net LOC, ~5–15 new tests.

| # | Name | Plan input | Difficulty | Mode | Est duration | Status |
|---|---|---|---|---|---|---|
| R1 | Foundation: persistent CODEX_HOME + group-id fallback (bug fix) | [10-phase-foundation.md](10-phase-foundation.md) | Medium | `/prex -ar` | 25–35 min | ✅ Done |
| R2 | Multi-account top-level CLI + account-aware path resolution | [11-phase-multiaccount-cli.md](11-phase-multiaccount-cli.md) | Medium | `/prex -a` | 25–35 min | ✅ Done |
| R3 | Quota reader + scoring selector | [12-phase-quota-selector.md](12-phase-quota-selector.md) | High | `/prex -ar` | 30–45 min | ⏳ Pending |
| R4 | Reactive 429 failover + retry-with-rotation + AuthBridge cleanup | [13-phase-failover.md](13-phase-failover.md) | Medium-High | `/prex -ar` | 30–40 min | ⏳ Pending |

**Do not collapse rounds together.** Combining R2+R3 would exceed the 8-step plan-review threshold and overflow Stage 3's 600 s Codex budget. R3+R4 share concerns (selector + retry) but separating them keeps the prex Stage-4 review surface manageable.

## Difficulty rationale

### R1 — Medium

Path-layout change touches everything downstream that resolves session paths (doctor, config_status, pass_through, cleanup, auth-bridge sites). Group-id resolution chain is straightforward. AuthBridge demotion is *removal* (easier than addition).

**Risks:**

- Breaking the 38 integration tests; hidden coupling on the `<root>/sessions/<terminal_id>` path inside test fixtures.
- `secure_dir` ownership/permission checks may reject some paths under `$XDG_STATE_HOME` if first-time creation order matters.

**Mitigations:**

- TestEnv (`tests/support/mod.rs`) provides hermetic XDG dirs — should adapt cleanly.
- `secure_dir` already handles the missing-parent case; re-test with new path nesting.

### R2 — Medium

Largely additive code (`services/account/`, `commands/account/`). The four-edit rule + `cli/profile.rs` template make the CLI shape mechanical. No `self` namespace work — just a top-level verb registration like the existing `profile` verb.

**Risks:**

- Help-rendering snapshot churn (`cmd_root_help`, `profile_help`, new `account_help`).
- `AccountId` newtype FromStr validation — table-driven test required for the regex.
- `cli/argv.rs::legacy_self_invocation` must be left in place (do NOT loosen the legacy `self` reject).

### R3 — High

New external dependency (`reqwest` blocking + rustls-tls), HTTP mocking infrastructure (`wiremock`), defensive JSON parser against an undocumented endpoint that has already changed shape (knightli warning), scoring formula with many table-driven test cases.

**Risks:**

- Schema drift in `wham/usage` response. Defensive parsing (raw fields, not derived labels) is essential.
- `cargo deny` / `cargo audit` reactions to new transitive deps.
- TLS feature flag: `rustls-tls` strongly preferred over `native-tls` (no OpenSSL link).

**Mitigations:**

- `wiremock` covers the schema-drift cases in tests.
- Pin `reqwest` to the latest stable + `cargo deny` config explicitly allows its license set.

### R4 — Medium-High

Tee-ing the child's stdout/stderr while preserving exit semantics is the only genuinely tricky part. Existing `Spawner` does fork/exec/wait without piping. Choose **post-wait scan** of captured buffer (simpler) unless TUI/long-running cases demand mid-flight detection — they don't, for the `codex exec` workload. Cleanup deletes ~250 LOC of AuthBridge watcher/signal.

**Risks:**

- Spawner refactor blast radius if streaming is needed.
- Signal-forwarding logic currently in `services/auth/signal.rs` may need to move to `commands/pass_through.rs` if the file is deleted.

**Mitigations:**

- Default to post-wait scan; if a real use case for mid-flight detection appears later, augment with a `PipingSpawner`.
- Move signal-forwarding code first as a separate step before deleting `auth/signal.rs`.

## Sequencing constraints

- **R1 → R2**: hard. R2's `registry.rs` writes to `<state-root>/accounts/<name>/`, which only exists once R1 has switched the path layout.
- **R2 → R3**: hard. `selector::pick` consumes an `AccountId` and the `Registry::list()` from R2.
- **R3 → R4**: soft. A stub `selector::pick` could exist earlier, but keep sequential for review-burden reasons.

Between rounds:

```sh
just check && git status   # confirm clean state before launching next prex
```

## End-to-end verification (post-R4)

```sh
# R1 — resume survives across invocations
codex-session exec --json "create a thread" > /tmp/r1.jsonl
THREAD=$(jq -r 'select(.type=="thread.started") | .thread_id' /tmp/r1.jsonl | head -1)
codex-session exec resume "$THREAD" --json "second turn"          # must succeed

# R2 — multi-account CLI
codex-session account add work --from-native
codex-session account add personal --from-native
codex-session account list
codex-session --account work exec "echo from work"                 # routed
ls ~/.local/state/codex-session/accounts/work/groups/
codex-session account use personal
codex-session exec "echo from personal"                            # routed via pin

# R3 — quota reader + selector
codex-session account quota --live --json | jq .
codex-session --account auto exec "echo balanced"                  # selector picks
codex-session account quota --all --json | jq .

# R4 — reactive failover
CODEX_SESSION_CHILD_BIN=$(realpath tests/fixtures/fake-429.sh) \
  codex-session --account auto --max-retries 2 exec "trigger 429"
codex-session account cooldown show --json
codex-session account cooldown clear --all

# Gates
just precommit-all                                                  # all hooks green
```

## "Done" criteria per round

Each round's done condition is uniformly:

```sh
just precommit-all   # → exit 0
```

…plus the round's specific manual smoke test (documented in `10-phase-foundation.md`, etc.). Prex Stage 4 review confirms the gate passes; Stage 5 (`-ar` rounds) runs an adversarial Codex review.

## Post-merge follow-ups (out of plan scope)

- TUI dashboard for `account quota` / `cooldown` (idea borrowed from `Soju06/codex-lb`).
- Desktop notifier ("weekly window at 80%").
- Encrypted vault for `auth.json` (caam Vault mode) — only if requested.
- Mid-session rotation (ndycode loopback proxy) — only if long-lived TUI cases emerge.
- Per-model quota tracking (the `wham/usage` payload exposes per-model windows; we only consume `five_hour` and `weekly` for now).

## How to launch each round

```sh
# Read the phase file content
ROUND_INPUT="$(cat .plan/multi-account-refactor/10-phase-foundation.md)"
# In Claude Code, invoke /prex (modes per the table above)
/prex -ar  $ROUND_INPUT
```

Each phase file is structured as a complete, self-contained task description suitable for direct ingestion by the `/prex` skill.
