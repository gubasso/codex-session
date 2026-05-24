# 99 — Execution Plan

## Phase grouping (one round = one `/prex` invocation)

Each round is a **single coherent goal** sized to one prex round (Stage 1 + Stage 3 each ≤600 s Codex budget). Per-round sizing target: 4–8 numbered plan steps, 1–5 files touched per step, ~300–700 net LOC, ~5–15 new tests.

| # | Name | Plan input | Difficulty | Mode | Est duration | Status |
|---|---|---|---|---|---|---|
| R1 | Foundation: persistent CODEX_HOME + group-id fallback (bug fix) | [10-phase-foundation.md](10-phase-foundation.md) | Medium | `/prex -ar` | 25–35 min | ✅ Done |
| R2 | Multi-account top-level CLI + account-aware path resolution | [11-phase-multiaccount-cli.md](11-phase-multiaccount-cli.md) | Medium | `/prex -a` | 25–35 min | ✅ Done |
| R3 | Quota reader + scoring selector | [12-phase-quota-selector.md](12-phase-quota-selector.md) | High | `/prex -ar` | 30–45 min | ✅ Done |
| R4 | Reactive 429 failover + retry-with-rotation + AuthBridge cleanup | [13-phase-failover.md](13-phase-failover.md) | Medium-High | `/prex -ar` | 30–40 min | ✅ Done (hardened by R4.5) |
| R4.5 | Hardening: interactive-TUI tee fix + cross-cutting fixes from R4 deep review | [14-phase-r4-hardening.md](14-phase-r4-hardening.md) | Medium | `/prex -ar` | 25–35 min | ✅ Done |
| R5 | Reintegrate Claude skills with the post-refactor wrapper API | [15-phase-skills-reintegration.md](15-phase-skills-reintegration.md) | Low | `/prex -ar` | 15–25 min | ✅ Done |

**Do not collapse rounds together.** Combining R2+R3 would exceed the 8-step plan-review threshold and overflow Stage 3's 600 s Codex budget. R3+R4 share concerns (selector + retry) but separating them keeps the prex Stage-4 review surface manageable. R4.5 must land before R5 because R5's smoke tests assume interactive `codex-session` works (B1 from the R4 review breaks that).

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

Tee-ing the child's stdout/stderr while preserving exit semantics is the only genuinely tricky part. Existing `Spawner` does fork/exec/wait without piping. R4 shipped with **post-wait scan** of a `wait_with_output()`-captured buffer (simpler); R4.5 then replaced that with a proper tee inside `spawn_and_wait_output` after the deep review flagged the buffered-stdio regression on the interactive Codex path. Cleanup deletes ~250 LOC of AuthBridge watcher/signal.

**Risks:**

- Spawner refactor blast radius if streaming is needed.
- Signal-forwarding logic currently in `services/auth/signal.rs` may need to move to `commands/pass_through.rs` if the file is deleted.

**Mitigations:**

- R4 shipped post-wait scan; R4.5 upgraded to true tee threads inside the spawner so live stdout/stderr forwarding survives. See [ADR D9](04-decisions.md) and [14-phase-r4-hardening.md](14-phase-r4-hardening.md) for the post-mortem and the chosen design.
- Move signal-forwarding code first as a separate step before deleting `auth/signal.rs`.

### R4.5 — Medium

Hardening round that acts on the R4 deep-review report:

- **B1 (blocking):** replace `wait_with_output()` with a tee inside `spawn_and_wait_output` so interactive Codex (the bare-`codex-session` path → TUI) keeps real-time stdout/stderr forwarding while still producing a captured buffer for `failover::scan`. Capture is capped at 1 MiB (`failover::MAX_CAPTURE_BYTES`).
- **I1–I6 (important):** JSON schema kebab-case parity, registry-aware cooldown path, drop module-scope `dead_code` allows, collapse the redundant `RegexSet` + `Vec<Regex>` double-pass, split `CooldownError::Parse` into `Decode`/`Encode`, short-circuit the pinned-account retry loop.
- **N1–N4 (nit):** harden the bash fixture, log on `i32 → u8` exit-code clamp, reject `cooldown clear --all --account X`, switch the test identity marker to an explicit `marker:` prefix.
- **Q1 (question):** add a SIGINT-mid-output regression test that locks in the tee fix's signal behavior.

**Risks:**

- Tee threads racing with `child.wait()` + signal forwarding (mitigated by joining the threads after `wait()` returns — the kernel keeps pipe contents readable past the writer's death).
- Print-ownership lint forbids `std::io::stdout()` calls outside `src/ui/**`, so the tee helper lives in `src/ui/raw_passthrough.rs::tee_to_stdio` rather than in `adapters/spawner.rs`.
- The pinned-account behavior change is **observable** — `tests/account_failover_pinned.rs` gets rewritten around the new short-circuit semantics. Call it out in the PR description.

**Mitigations:**

- Each fix is independent and individually revertible; if the tee design regresses signal handling, only step 2 of the R4.5 plan needs to be undone.
- The new test `tests/cmd_signal_passthrough_long_output.rs` runs every CI build going forward.

## Sequencing constraints

- **R1 → R2**: hard. R2's `registry.rs` writes to `<state-root>/accounts/<name>/`, which only exists once R1 has switched the path layout.
- **R2 → R3**: hard. `selector::pick` consumes an `AccountId` and the `Registry::list()` from R2.
- **R3 → R4**: soft. A stub `selector::pick` could exist earlier, but keep sequential for review-burden reasons.
- **R4 → R4.5**: hard. R4.5 cannot land before R4 because every fix targets R4's surface (the new `spawn_and_wait_output`, the new `failover::scan`, the new `cooldown` module, the new `account cooldown` CLI). R4.5 must land before any interactive validation in R5 — the B1 fix is the reason `codex-session` (bare-TUI mode) is usable in the first place.
- **R4.5 → R5**: hard. R5 hand-runs `codex-session exec` smoke tests against the wrapper; B1 makes those tests fail by buffering all output until the agent exits.

Between rounds:

```sh
just check && git status   # confirm clean state before launching next prex
```

## End-to-end verification (post-R4.5)

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

# R4.5 — interactive TUI tee + cross-cutting fixes
codex-session                                                       # TUI: live prompts + color
CODEX_SESSION_CHILD_BIN=$(realpath tests/fixtures/fake-429.sh) \
  codex-session --account work --max-retries 2 exec "hi" 2>&1 | grep "ignored"
ls ~/.local/state/codex-session/accounts/work/cooldown.json         # must NOT exist (short-circuit)
codex-session account cooldown show --json | jq 'first | has("cooled-down")'   # → true
codex-session --account work account cooldown clear --all 2>&1 | grep "conflicts with"
CODEX_SESSION_ACCOUNT_REGISTRY_DIR=/tmp/custom-accounts \
  codex-session --account auto --max-retries 1 exec "trigger 429" || true
ls /tmp/custom-accounts/*/cooldown.json                             # must exist

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
