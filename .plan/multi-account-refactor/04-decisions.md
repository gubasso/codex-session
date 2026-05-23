# 04 — Architecture Decision Records

Eight ADRs capture the load-bearing design choices. Each ADR is a stable reference — if a future change wants to revisit one, log a new ADR rather than editing in place.

---

## D1 — Persistent `CODEX_HOME` (not ephemeral per-call)

**Status:** Accepted.
**Date:** 2026-05-22.

### Context

Pre-refactor, every `codex-session` invocation provisioned a fresh `CODEX_HOME` under `$XDG_RUNTIME_DIR/codex-session/sessions/<terminal_id>/`, with `terminal_id` silently falling back to `pid-{getpid()}` in headless contexts. Upstream codex stores rollouts under `$CODEX_HOME/sessions/.../rollout-*.jsonl` and resolves `codex exec resume <thread-id>` via a state DB backfilled from that tree. Different `CODEX_HOME` → resume can never find the prior rollout. Verified empirically: every headless invocation (Claude-Code child, `/prex`, `/review-loop`) created a new `pid-N/` dir; resume always failed.

### Decision

Replace ephemeral per-call `CODEX_HOME` with a **persistent** dir under `$XDG_STATE_HOME/codex-session/` (not `$XDG_RUNTIME_DIR`, which is tmpfs / wiped on logout). The exact path is keyed on the resolved `(account, group_id)` pair (see [D2](#d2-two-axis-account--group_id-layout)).

### Consequences

- Native `codex exec resume` works across invocations — fixes the immediate `/prex` round-2 failure.
- State accumulates on disk; pruning is needed for legacy `pid-*` dirs (handled in R1) but the new layout's persistence is intentional — it's the storage codex assumed all along.
- Multi-terminal isolation is preserved by the `group_id` axis.

### Alternatives considered

- **Share user's real `~/.codex`** (no isolation): rejected — loses the design goal of multi-terminal isolation; multiple parallel invocations would pollute shared prompt history / memories.
- **Symlink rollouts dir back to a shared location**: rejected — codex's resume requires more than rollouts (state DB, session_index, memories). Symlinking only `sessions/` doesn't solve it.
- **Mid-session rotation proxy** (ndycode-style): rejected — out of scope; orthogonal problem for long-lived TUI use, not our `codex exec` workload.

### References

- [02-references.md](02-references.md) (upstream `find_thread_path_by_id_str` proves CODEX_HOME-rooted resolution)
- [03-inspired-projects.md](03-inspired-projects.md): `Ducksss/codex-profiles`, `Spielewoy/multi-codex` (validate the per-account `CODEX_HOME` pattern)

---

## D2 — Two-axis (account × group_id) layout

**Status:** Accepted.
**Date:** 2026-05-22.

### Context

A single-axis layout — `accounts/<account>/CODEX_HOME` — suffices for multi-account but does not solve resume in the common headless case where one user has one account but runs many parallel terminal sessions. Conversely, a single-axis `groups/<group_id>/CODEX_HOME` solves resume but doesn't extend to multi-account. We need both axes orthogonally.

### Decision

`<state-root>/accounts/<account>/groups/<group_id>/` is the path passed to upstream codex as `$CODEX_HOME`.

- `<account>` defaults to `"default"` until R2; multi-account from R2 on.
- `<group_id>` resolves via the 5-step chain documented in [01-architecture.md](01-architecture.md). Stable across invocations from the same terminal / parent / env.

### Consequences

- Each terminal × account combination gets its own `CODEX_HOME`. Resume works within a (terminal, account) pair. Different terminals are isolated; different accounts are isolated.
- The multi-account selector + load balancer (R3+) operates entirely on the account axis; the group axis is invisible to it.
- Storage growth is bounded by (active accounts) × (active terminals); a cleanup pass can prune (account, group_id) combos with no recent activity.

### Alternatives considered

- **Single account axis** (`<state-root>/accounts/<account>/`): loses cross-invocation terminal isolation within one account. Two parallel terminals on the same account would contend on `state_5.sqlite` and share prompt history.
- **Single group axis** (`<state-root>/groups/<group_id>/`): blocks the multi-account extension entirely.
- **Session-group axis nested inside account, but with `account` collapsed away**: same as single-group axis.

### References

- [03-inspired-projects.md](03-inspired-projects.md): no reviewed project solves the (account × group) two-axis problem. caam / ndycode / Loongphy / codex-profiles all collapse to one axis.

---

## D3 — Retire `AuthBridge` (one-shot importer only)

**Status:** Accepted (R1 demote, R4 delete).
**Date:** 2026-05-22.

### Context

Pre-refactor `AuthBridge` watched native `~/.codex/auth.json` mid-flight and persisted any refreshed token back on drop. This was needed when CODEX_HOME was ephemeral — without persistence, a token refreshed by codex would be lost when the ephemeral dir vanished. With persistent per-account `CODEX_HOME`, codex refreshes tokens *in place* in the account's own `auth.json`; nothing needs to flow back to native.

### Decision

- R1: keep only `AuthBridge::import_if_missing(group_dir, native_home)` — if the account-group has no `auth.json` yet, seed it from native once. Unwire watcher + persist-on-drop from `pass_through.rs`. Sub-modules `auth/watcher.rs`, `auth/signal.rs` stay compiled but unused.
- R4: delete `auth/watcher.rs`, `auth/signal.rs`, the `last_refresh` timestamp comparison, and associated tests. ~250 LOC removed.

### Consequences

- Eliminates the entire token-refresh write-back race surface (which caam, Loongphy, and the `Sls0n`/`bashar94`/`denysdovhan` swappers all chase or leave unfixed).
- Simpler mental model: each account's `auth.json` is the source of truth for that account. Native `~/.codex/auth.json` is now used only as a *seed* for the very first account.
- `codex-session account add --from-native` becomes the explicit, opt-in seeding action (R2).

### References

- `.plan/codex-session-multi-account-research.md` §5 (token-refresh write-back race documented in source)
- [03-inspired-projects.md](03-inspired-projects.md): the swap-based tools (Tier-C) demonstrate why this race must be sidestepped.

---

## D4 — No rotation proxy / no resident daemon

**Status:** Accepted.
**Date:** 2026-05-22.

### Context

`ndycode/codex-multi-auth` adds runtime mid-session rotation via a loopback Responses proxy. `Soju06/codex-lb` is a FastAPI rotation-as-a-service. Both add significant infrastructure (daemons, named-binary wrappers, plugin systems) that fight the transparent-wrapper invariant codex-session is built on.

### Decision

Codex-session stays a transparent fork/exec wrapper. Between-invocation rotation only (driven by the selector + reactive failover). No daemons, no proxies, no plugin infrastructure, no renamed binary.

### Consequences

- Codex-session keeps its current "type a verb, the child runs" UX. No surprise process tree.
- Mid-session rotation (e.g., the long-lived TUI hits 429 at minute 47) is out of scope. If this need emerges, revisit by adopting `ndycode`'s pattern as an opt-in subcommand (`codex-session proxy run`), not as a default.
- The `account` subcommand surface stays small and POSIX-shaped.

### References

- [05-cli-design.md](05-cli-design.md) — wrapper-design conformance (the §6 cardinal rule: "default to verbatim pass-through; translate only when you must").

---

## D5 — Inline Rust quota reader (not shell-out to `Loongphy/codex-auth`)

**Status:** Accepted.
**Date:** 2026-05-22.

### Context

The research plan §7.2 considered two options for reading per-account quota: (a) shell out to `Loongphy/codex-auth` (Zig binary, 1.7k★, calls `/backend-api/wham/usage` correctly), or (b) re-implement inline (~200 LOC).

### Decision

Inline Rust implementation via `reqwest` blocking client. ~200 LOC + tests.

### Consequences

- No extra external dependency to install / version-pin / vendor. `codex-session` stays self-contained.
- HTTP-mocking via `wiremock` gives deterministic tests for schema-drift cases.
- We own the parser, so we can apply knightli's "parse raw fields, not derived labels" warning end-to-end.

### Alternatives considered

- **Shell out to `codex-auth`:** more code (process invocation, JSON IPC, version compat shim) for less control. Tests would need a recorded fixture binary.

### References

- [06-quota-protocol.md](06-quota-protocol.md) — endpoint / headers / parsing spec.
- `.plan/codex-session-multi-account-research.md` §4 — endpoint shape + knightli warning.

---

## D6 — Borrow caam's regex 429 detector (verbatim)

**Status:** Accepted.
**Date:** 2026-05-22.

### Context

`Dicklesworthstone/caam` (Go, 124★) wraps tools and detects 429s by scanning the wrapped child's stdout/stderr with a regex pattern. The patterns are battle-tested across multi-vendor (Codex, Claude, Gemini) output formats. caam's source is permissively licensed (MIT-ish).

### Decision

Copy caam's Codex detector patterns verbatim as a six-pattern set:

- `(?i)rate.?limit`
- `(?i)quota.?exceeded`
- `\b429\b`
- `(?i)too.?many.?requests`
- `(?i)exceeded.*rate`
- `(?i)slow.?down`

Cite the source URL in the code comment. Implement as a passive observer in `services/account/failover.rs` (R4): tee the child's stdout/stderr through unmodified; record matches.

### Consequences

- One less design decision to debate. Pattern is empirically validated.
- Easy to extend if a new provider surfaces a new phrase — but per the cli-spec "parse don't validate" rule, extensions go through `caam`'s own evolution if possible (or a new test case here).

### References

- caam source: `raw.githubusercontent.com/Dicklesworthstone/coding_agent_account_manager/refs/heads/main/internal/ratelimit/detector.go`
- [07-failover-spec.md](07-failover-spec.md) — full pattern + cooldown schema.

---

## D7 — Borrow caam's scoring formula (verbatim)

**Status:** Accepted.
**Date:** 2026-05-22.

### Context

The proactive selector needs to rank accounts. caam's `internal/rotation/rotation.go` has a multi-factor scoring formula (health, penalty, plan, recency, real-time availability) that's been tuned in production against a mix of providers.

### Decision

Adopt caam's scoring formula verbatim in `services/account/selector.rs::pick()`:

- Cooldown → disqualifying.
- Health bonus: `+100` healthy / `+50` degraded / `-50` unhealthy.
- Penalty: `-(penalty * 10)`.
- Plan bonus: `+30` enterprise / `+20` pro|team / `0` free.
- Recency: penalty for recent use, reward for long-idle (LRU).
- Real-time availability: `avail_score - 50`, additional `-30` if `weekly.percent_left < 20.0`.
- Threshold gate: `five_hour.percent_left > 50.0 && weekly.percent_left > floor`.

### Consequences

- Same justification as D6 — pre-validated, less bikeshedding.
- Floor / threshold are configurable via `[account]` config; the *formula* is fixed.

### References

- caam source: `raw.githubusercontent.com/Dicklesworthstone/coding_agent_account_manager/refs/heads/main/internal/rotation/rotation.go`

---

## D8 — Top-level `account` verb (no `self` prefix)

**Status:** Accepted.
**Date:** 2026-05-22.
**Reverses:** an earlier draft of this plan that placed account verbs under `self`.

### Context

An earlier draft proposed `codex-session self account add/list/...` as the CLI surface, modeled loosely on `rustup self update`. User-review against the project's CLI design SoT (`tech/programming/cli-design/06-cli-wrapper-design/process-and-posix.md`) revealed this conflicts with §5.1:

> The `self` rule — narrow, not generic. A common mistake is to treat `self` as a namespace for *every* wrapper-owned verb. … `self` is justified only when (1) the verb's object is the running binary itself — it updates, uninstalls, or otherwise mutates the wrapper, AND (2) the same verb name plausibly exists on the wrapped child.

Stripping `self` mentally from `self account list` yields `account list`. Still unambiguous; codex's verb namespace has no `account`. Therefore `self` is pure noise. §5.3 surveys 9 established wrapper CLIs (`gh`, `kubectl`, `gcloud`, `git`, `cargo`, `op`, `flyctl`, `rustup`, `uv`) and confirms: only `rustup` and `uv` use `self`, and only for self-mutating verbs (`self update`, `self uninstall`).

### Decision

All new wrapper-owned subcommands live at the **top level**. `codex-session account add/list/current/use/remove/quota/cooldown`. This also matches the precedent the wrapper already set with its existing `version` / `completion` / `config` / `profile` / `doctor` verbs.

### Consequences

- Shorter, idiomatic CLI surface. Aligns with `gh auth`, `op account`, `gcloud auth`.
- No need to touch `src/cli/argv.rs::legacy_self_invocation` — the existing reject-`self` contract stays in place, preventing accidental `self <verb>` typos from being silently accepted.
- One collision risk: if upstream codex adds an `account` verb someday, the wrapper would shadow it (same model as the existing `doctor`/`completion` shadows). Documented in `docs/upstream-codex.md`; monitor upstream releases.

### References

- `/home/gu/Projects/docs-n-notes/tech/programming/cli-design/06-cli-wrapper-design/process-and-posix.md` §5.1, §5.2, §5.3
- [05-cli-design.md](05-cli-design.md) — full subcommand-tree design

---

## D9 — Tee-based stdio for `Spawner::spawn_and_wait_output`

**Status:** Accepted.
**Date:** 2026-05-22.
**Supersedes:** the implicit decision behind R4's `spawn_and_wait_output`, which used `Stdio::piped()` + `child.wait_with_output()` and buffered all child output until exit.

### Context

R4 added `spawn_and_wait_output` so `failover::scan` could inspect captured child output for 429 patterns. The first implementation set `stdout`/`stderr` to `Stdio::piped()` and called `child.wait_with_output()`, deferring every byte to the parent terminal until the child exited.

The R4 deep-review (Stage 4 of `/prex -ar`) flagged this as a blocker. Three compounding regressions:

1. **Interactivity:** the bare `codex-session` invocation routes through `dispatch::run` → `pass_through::run` → `retry::run_with_retry` → `run_once`. With piped stdio, prompts, streaming responses, and live feedback are invisible until the Codex TUI quits — i.e. the primary user-facing path is broken.
2. **TTY detection:** the child sees `stdout`/`stderr` as pipes (`isatty(fd) == 0`), disabling its TUI/color machinery.
3. **Memory:** the captured `Vec<u8>` grows with the entire session's output; unbounded for long-lived runs.

The pre-R4 path used inherited stdio with no capture, but that has no failover signal.

### Decision

`Spawner::spawn_and_wait_output` **tees** each pipe with a dedicated reader thread. Per-stream loop:

```
read 8 KiB chunk from child pipe
    ├── write to parent's real stdout/stderr (live)
    └── append to capture Vec<u8> (until MAX_CAPTURE_BYTES)
```

After `child.wait()` returns, the spawner joins the two reader threads (the kernel keeps pipe contents readable past the writer's death, so EOF is reached cleanly) and returns `ChildOutput { status, stdout: captured_stdout, stderr: captured_stderr }`.

The tee helper lives in `src/ui/raw_passthrough.rs::tee_to_stdio`, NOT in `src/adapters/spawner.rs`, because the project's `lint-print` recipe forbids `std::io::stdout()` / `std::io::stderr()` calls outside `src/ui/**`, `src/error.rs`, `src/logging.rs`, and tests. The spawner calls into `tee_to_stdio` instead of taking the stdio lock directly.

`MAX_CAPTURE_BYTES = 1 << 20` (1 MiB) caps the capture buffer. Live forwarding stays unbounded — only the capture half stops appending. The detector scans line-by-line, so truncation at a non-line boundary is safe (the next line just doesn't get matched). Failover decisions don't need to see the last megabyte of a long session.

### Consequences

- **Restored interactivity:** real-time output for every pass-through invocation, including the bare-`codex-session` TUI path.
- **TTY trade-off remains:** the child still sees pipes (not a real TTY). This is the unavoidable cost of needing capture; the established workaround is `CODEX_FORCE_TTY=1` (or the equivalent env Codex exposes) when interactive TUI mode is wanted. Documented in `07-failover-spec.md`.
- **Bounded memory:** capture stays under 1 MiB regardless of session length.
- **Signal handling:** the existing `install_signal_forwarding` thread keeps working — the reader threads handle `BrokenPipe` as "stop forwarding, keep capturing" so a SIGINT'd child still produces a clean buffer.
- **Lint surface unchanged:** the print-ownership whitelist does not gain `adapters/` (preserves a load-bearing invariant).

### Alternatives considered

- **Fast-path bypass when `max_retries == 0`:** keep the old inherited-stdio `spawn_and_wait` for the common path and only switch to piped when failover is opted in. **Rejected** — bifurcates the spawn path, so any future selector improvement that wants to scan output (e.g., quota-error detection beyond 429) would have to wire the same lifecycle twice. Consistent behavior across modes is worth the +30 LoC.
- **Add `src/adapters/spawner.rs` to the print-ownership lint whitelist:** lets the spawner call `std::io::stdout()` directly. **Rejected** — erodes a load-bearing invariant; every future stdio call in the spawner becomes unflagged.
- **Streaming detector that runs on each chunk:** would catch 429 mid-flight and let the wrapper kill the child early. **Out of scope** for R4.5 (and beyond) — post-wait scan is sufficient for the `codex exec` workload; long-lived TUI mid-flight 429 is an open follow-up.

### Addendum (R4.5 review-loop round 4)

- **PID-clear timing.** `spawn_and_wait_output` clears the shared `pid_sink` AtomicI32 to `0` immediately after `child.wait()` returns, *before* joining the tee threads. Without this, a signal delivered during the tee-thread drain window could be forwarded to a recycled PID. The caller (`pass_through::run_child`) still does a belt-and-suspenders second clear after the spawner returns.
- **Detector input.** `pass_through::run_once` returns `(exit_code, stdout_buf, stderr_buf)` as three independent values; the retry harness runs `failover::scan` on stderr first (where Codex emits its rate-limit diagnostics) and falls back to stdout, instead of concatenating the two streams. Concatenation manufactured a synthetic line boundary that could match across a `stdout`-tail / `stderr`-head fragment (false positive) or reorder real interleavings out of match shape (false negative).

### Addendum (R4.5 review-loop round 5)

- **One signal-forwarder per wrapper invocation.** `commands/pass_through::SignalSession` owns the shared `Arc<AtomicI32>` for the live child PID plus the once-installed `SignalGuard`. `retry::run_with_retry` installs it once before the attempt loop; every attempt's `run_once`/`run_child` reuses the same guard. Installing a fresh forwarder per attempt would leak iterator threads whose stale `child_pid` Arcs (cleared to `0` after each reap) would take the "no child yet" branch on a later signal and call `emulate_default_handler(sig)`, terminating the wrapper instead of forwarding to the live child. `single_attempt` installs its own one-shot session.

### References

- R4 deep-review finding B1 — see `.plan/multi-account-refactor/14-phase-r4-hardening.md` (R4.5 prex input) for the full reasoning.
- R4.5 implementation: `src/adapters/spawner.rs::spawn_and_wait_output`, `src/ui/raw_passthrough.rs::tee_to_stdio`, `src/services/account/failover.rs::MAX_CAPTURE_BYTES`.
- [07-failover-spec.md](07-failover-spec.md) — updated detection-placement section.
