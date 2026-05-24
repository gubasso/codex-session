# Round 4.5 — Hardening after the R4 deep review

This file is the **prex input** for Round 4.5. Pass its contents verbatim to `/prex -ar`.

---

## Prerequisite

Round 4 has landed (PR with the failover detector, cooldown service, retry harness, `account cooldown` CLI, and AuthBridge cleanup). `just precommit-all` exits 0 on the working tree. A `review-code-deep` pass over the R4 diff produced one [blocking] finding (B1), six [important] (I1–I6), four [nit] (N1–N4), and one open [question] (Q1). This round addresses all of them.

The blocking finding (B1) is a primary-use-case regression: the new `Stdio::piped()` + `wait_with_output()` flow in `pass_through::run_once` breaks **every** interactive `codex-session` invocation (TUI mode, live progress, color). R4's integration tests didn't catch it because their fixtures all exit in <100 ms; in normal use the bug is severe. R4.5 fixes it.

## Goal

- **B1**: restore real-time stdout/stderr forwarding for every pass-through invocation while keeping the captured byte buffer that `failover::scan` needs.
- **I1**: make the `account cooldown` JSON schema kebab-case (matches every other `account` JSON view).
- **I2**: route the cooldown path through `Registry::account_dir(...)` so the custom `account.registry_dir` knob keeps working.
- **I3**: drop module-scope `#![allow(dead_code)]` from `failover.rs` and `cooldown.rs`; tighten visibility where appropriate.
- **I4**: collapse the redundant `RegexSet` + `Vec<Regex>` double-pass in `failover::scan` to a single `RegexSet::matches` call that also records *which* pattern matched.
- **I5**: rename `CooldownError::Parse` to disambiguate read-side decode vs. write-side encode failures.
- **I6**: fix the misleading "no-ops" warning text on pinned-account retry, and short-circuit the retry loop so pinned accounts don't write cooldowns or run extra child invocations.
- **N1–N4**: harden the test fixture (`set -euo pipefail`, explicit identity marker), log on `i32 → u8` exit-code truncation, and reject `cooldown clear --all --account X` as a usage error.
- **Q1**: add a SIGINT mid-output regression test so the tee fix from B1 keeps working when the child is interrupted before exit.

## Background (read before planning)

- `.plan/multi-account-refactor/07-failover-spec.md` — failover spec; **R4.5 updates this** (replace the "post-wait scan" rationale with the tee strategy, replace the invented regex matrix with the actual caam Codex pattern list).
- `.plan/multi-account-refactor/04-decisions.md` — ADRs. R4.5 appends **D9** (tee-based stdio for `spawn_and_wait_output`).
- `.plan/multi-account-refactor/13-phase-failover.md` — R4's prex input. Do not edit; keep it as the historical record of what R4 shipped.
- The R4 deep-review report is in the orchestrator transcript; the findings are restated below in this doc (so this file is self-contained for `/prex`).
- caam detector source (for cross-checking the regex set): `raw.githubusercontent.com/Dicklesworthstone/coding_agent_account_manager/refs/heads/main/internal/ratelimit/detector.go`. The R4 implementation correctly uses the six-pattern Codex list; do **not** revert to the spec's prior invented alternation.

## Findings reference (verbatim restatement of the deep review)

### B1 — Buffered piped stdio breaks interactive Codex TUI

- **Files:** `src/adapters/spawner.rs:270-292` (`spawn_and_wait_output`), `src/commands/pass_through.rs:184-211` (`run_child`).
- **Reasoning:** `cmd.stdout(Stdio::piped())` + `child.wait_with_output()` defers ALL bytes to the parent terminal until the child exits. Combined with `dispatch.rs:33` routing the bare `codex-session` (no subcommand → primary interactive entry to the Codex TUI) through `pass_through::run` → `retry::run_with_retry` → `run_once`, this breaks:
    1. **Interactivity:** prompts, streaming responses, and live feedback are invisible until the user quits.
    2. **TTY detection:** the child sees `stdout`/`stderr` as pipes; it disables TUI (`isatty(fd) == 0`), suppresses color (`stderr_color()`), and likely falls back to non-interactive output.
    3. **Memory:** the captured `Vec<u8>` grows with total child output; unbounded for long interactive sessions.
- **Severity:** blocking.

### I1 — JSON output schema inconsistency

- **Files:** `src/commands/account/mod.rs:65-79`, `tests/account_cooldown_cli.rs:76,78`.
- **Reasoning:** every existing account view uses `#[serde(rename_all = "kebab-case")]` (e.g. `last_used_at_unix → "last-used-at-unix"`). `AccountCooldownEntryView` uses `"snake_case"`. A consumer running `account list --format json | jq` and `account cooldown show --json | jq` has to switch convention between the two. The R4 test asserts the snake form (`items[0]["cooled_down"]`), so the contract gets locked in.
- **Severity:** important. JSON schema fix is a one-line change but irreversible once shipped.

### I2 — Cooldown path diverges from registry under custom `account.registry_dir`

- **Files:** `src/services/account/cooldown.rs:33-38` (`path` uses `state_root/accounts/<id>/cooldown.json`), `src/services/account/registry.rs:25-40` (registry honors `config.account.registry_dir`), `src/config/mod.rs:71,498` (the config knob + env mirror `CODEX_SESSION_ACCOUNT_REGISTRY_DIR`).
- **Reasoning:** Pre-R4 `selector.rs::cooldown_active` (now deleted) called `registry.account_dir(id).join("cooldown.json")` and so honored `registry_dir`. Post-R4 the cooldown service hardcodes `state_dir/accounts/<id>`. If a user sets `registry_dir = "/custom/accounts"`, accounts live at `/custom/accounts/<id>/` but `cooldown::write` targets the parent-less `state_dir/accounts/<id>/cooldown.json` and fails with `NotFound` → exit 74. Selector reads silently miss.
- **Severity:** important.

### I3 — Module-scope `#![allow(dead_code)]` hides regressions

- **Files:** `src/services/account/failover.rs:16`, `src/services/account/cooldown.rs:1`.
- **Reasoning:** module-scope allows future symbols to silently rot — a function that becomes unused after a refactor escapes clippy. After audit, `failover.rs` has no actually-dead items; `cooldown.rs` shields only `path()`, which is `pub(crate)` with no external callers (could be private).
- **Severity:** important.

### I4 — `PATTERNS` + `MATCHERS` is a redundant double-pass

- **File:** `src/services/account/failover.rs:22-63`.
- **Reasoning:** `RegexSet::is_match` is by definition `OR` of its members; building a parallel `Vec<Regex>` from the same `PATTERNS_RAW` and asking it `.any(|r| r.is_match(...))` is mathematically guaranteed to return `true` whenever the `RegexSet` already matched. Pure CPU waste + confusing intent. If the goal was to expose *which* pattern matched, the `Match` struct should carry an index or name; instead it only has `{line_no, snippet}`.
- **Severity:** important.

### I5 — `CooldownError::Parse` is misnamed

- **File:** `src/services/account/cooldown.rs:19-24,60-63`.
- **Reasoning:** the same `Parse` variant is raised from `serde_json::from_slice` (true parse) and from `serde_json::to_vec_pretty` (serialize). A serializer failure surfacing as `"cooldown parse failed at <path>"` misleads forensic readers and grep-based alerts.
- **Severity:** important.

### I6 — Pinned-account warning text wrong + write-amplification

- **Files:** `src/services/account/retry.rs:14-26`, `tests/account_failover_pinned.rs:24-34`.
- **Reasoning:** the warning says `"retries with a pinned --account are no-ops; use --account auto for failover"`, but the wrapper actually runs `N+1` child invocations and writes `N` cooldown files for an account that will never be skipped (because it's pinned). That's write-amplification with no failover benefit. The warning text is inaccurate; the behavior wastes effort; the cooldown JSON is misleading (the account is "cooled down" but the user pinned it, so it'll be picked again anyway).
- **Severity:** important.

### N1 — Bash fixture missing `set -euo pipefail`

- **File:** `tests/fixtures/fake-429.sh:1-6`.
- **Severity:** nit. Three lines, hard to break today; future-proof.

### N2 — `child_exit_code` truncates `i32 → u8` silently

- **File:** `src/commands/dispatch.rs:36-38`.
- **Reasoning:** POSIX shells already truncate to low 8 bits, so the clamp is consistent. But a child that exits with e.g. 300 (via `_exit(300)`) lands at 255 with no breadcrumb in the wrapper logs.
- **Severity:** nit.

### N3 — Fixture identity marker brittle

- **Files:** `tests/fixtures/fake-429.sh:4`, `tests/account_failover_retry.rs:71`, `tests/account_failover_pinned.rs:28`.
- **Reasoning:** tests `split("codex_home=")` to discover which account each attempt used. Renaming the env var anywhere in `pass_through.rs` breaks the tests with a misleading diff.
- **Severity:** nit.

### N4 — `clear --all --account X` silently ignores `--account X`

- **File:** `src/commands/account/cooldown.rs:54-69`.
- **Reasoning:** `--all` wins over `--account` with no warning. Clap can't express the conflict natively because `--account` is the global flag and `--all` is subcommand-local; the check must be manual.
- **Severity:** nit.

### Q1 — Does `wait_with_output()` drain stdout/stderr when child is SIGINT'd mid-write?

- **File:** `src/adapters/spawner.rs:286`, interaction with `install_signal_forwarding`.
- **Reasoning:** `wait_with_output()` spawns internal pipe-drain threads; on a forwarded SIGINT the child is killed, then the pipe contents drain. Should work, but no regression test exists.
- **Severity:** question — becomes a test that locks in the behavior once the B1 tee fix lands.

## Numbered implementation steps

1.  **Pre-flight verifications.**
    Re-confirm against the current code before starting (the deep review captured this state but any intervening commit may have drifted):

    1. `src/adapters/spawner.rs` exposes both `spawn_and_wait` (inherited stdio, returns `ExitStatus`) and `spawn_and_wait_output` (piped stdio, returns `ChildOutput { status, stdout, stderr }`). Verify with `rg -n 'fn spawn_and_wait' src/adapters/spawner.rs`.
    2. `pass_through::run_child` calls `spawn_and_wait_output` unconditionally. Verify with `rg -n 'spawn_and_wait_output' src/commands/pass_through.rs`.
    3. `failover::scan` runs the `PATTERNS` `RegexSet` then a redundant `MATCHERS` `Vec<Regex>` loop. Verify with `rg -n 'PATTERNS|MATCHERS' src/services/account/failover.rs`.
    4. `AccountCooldownEntryView` is `#[serde(rename_all = "snake_case")]`; every other `AccountXxxView` is `kebab-case`. Verify with `rg -n 'rename_all' src/commands/account/mod.rs`.
    5. `cooldown::path` accepts `state_root: &Utf8Path` and joins `accounts/<id>/cooldown.json` literally; does NOT call into `Registry`. Verify with `rg -n 'fn path' src/services/account/cooldown.rs`.
    6. `account.registry_dir` config field still exists and the `CODEX_SESSION_ACCOUNT_REGISTRY_DIR` env override is still wired. Verify with `rg -n 'registry_dir' src/config/mod.rs`.
    7. `tests/fixtures/fake-429.sh` does not have `set -euo pipefail` and emits `codex_home=` as its only identity marker. Verify with `cat tests/fixtures/fake-429.sh`.

    If any of these has drifted (e.g., someone else fixed B1 in the meantime), stop and re-scope the round before continuing.

2.  **Fix B1: tee-based `spawn_and_wait_output` in `src/adapters/spawner.rs`.**

    The chosen strategy is a tee inside the spawner adapter: spawn two short-lived reader threads, each pulling bytes off the child's `stdout`/`stderr` pipe and (a) forwarding them to the parent's real stdout/stderr **in real time** and (b) appending them to per-stream `Vec<u8>` buffers for `failover::scan`. After the threads join, return the captured `ChildOutput`.

    Public surface in `src/adapters/spawner.rs` (unchanged signature, new body):

    ```rust
    fn spawn_and_wait_output(
        &self,
        inv: ChildInvocation,
        pid_sink: &AtomicI32,
    ) -> Result<ChildOutput, SpawnerError>;
    ```

    Implementation sketch (load-bearing details):

    ```rust
    use std::process::Stdio;
    use std::io::{Read as _, Write as _};
    use std::thread;

    let mut cmd = inv.into_command();
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(SpawnerError::Exec)?;
    pid_sink.store(i32::try_from(child.id()).unwrap_or(i32::MAX), Ordering::SeqCst);

    let mut child_stdout = child.stdout.take().expect("piped");
    let mut child_stderr = child.stderr.take().expect("piped");

    let stdout_handle = thread::spawn(move || {
        let mut buf = Vec::new();
        let mut sink = std::io::stdout().lock();
        copy_tee(&mut child_stdout, &mut sink, &mut buf)?;
        std::io::Result::Ok(buf)
    });
    let stderr_handle = thread::spawn(move || {
        let mut buf = Vec::new();
        let mut sink = std::io::stderr().lock();
        copy_tee(&mut child_stderr, &mut sink, &mut buf)?;
        std::io::Result::Ok(buf)
    });

    let status = child.wait().map_err(SpawnerError::Exec)?;
    let stdout = stdout_handle.join().map_err(...)??;
    let stderr = stderr_handle.join().map_err(...)??;
    Ok(ChildOutput { status, stdout, stderr })
    ```

    The `copy_tee` helper is a small `pub(crate) fn copy_tee(src: &mut impl Read, sink: &mut impl Write, capture: &mut Vec<u8>) -> std::io::Result<()>` that loops over an 8 KiB chunk buffer, writes to `sink` (flushing if it's a TTY) and extends `capture`. Live by default; the lock on the parent stdio is per-chunk so the wrapper's own stderr (warnings, color) can interleave without garbling.

    Removal: the `pass_through.rs` post-wait `write_raw(...)` calls become **no-ops** for the live-forwarding axis, because the tee thread already streamed the bytes. Either:

    - **Option A (preferred):** drop the two `crate::ui::raw_passthrough::write_raw(...)` calls in `pass_through::run_child` (lines 198-205 today). `raw_passthrough` still has a use case (potential dry-run echo, future debugging), so keep the module but `#[allow(dead_code)]` is fine if no other caller appears in this round.
    - **Option B:** keep `raw_passthrough` and route the tee through it. Strictly more code; reject unless print-ownership lint pushes back on the new thread bodies in `adapters/spawner.rs`.

    **Lint risk:** `just lint-print` forbids direct `std::io::stdout()` / `std::io::stderr()` outside `src/ui/**`, `src/error.rs`, `src/logging.rs`, and tests. `adapters/spawner.rs` is NOT on the whitelist. Two options, in order of preference:

    1. **Move `copy_tee` into `src/ui/raw_passthrough.rs`** as `pub(crate) fn tee_to_stdio(stream: RawStream, src: &mut impl Read, capture: &mut Vec<u8>) -> io::Result<()>` and have the spawner call it. Keeps the lint whitelist intact; `raw_passthrough` finally has a non-trivial reason to exist.
    2. **Add `adapters/spawner.rs` to the lint whitelist** (see `justfile::lint-print`). Justify with a comment block. Less work; uglier blast radius (every future stdio call in the spawner is now unflagged).

    Pick option (1). Update `src/ui/raw_passthrough.rs` to export `tee_to_stdio` (keep `write_raw` for the dry-run / future-debug paths; remove the `#![allow(dead_code)]` once `tee_to_stdio` is in use).

    **Memory cap:** the captured `Vec<u8>` is still unbounded in pathological cases (a child that writes 10 GB before exiting). Add a `MAX_CAPTURE_BYTES = 1 << 20 // 1 MiB` constant in `failover.rs` and have `tee_to_stdio` stop appending to `capture` past that limit (it keeps forwarding to the real sink). Document the choice: failover scanning only needs to see the first 1 MiB of output; the last megabyte of a long session is irrelevant to a 429 decision. The detector already scans line-by-line, so a buffer truncated at a non-line-boundary is safe (the next line just doesn't get matched).

3.  **Fix Q1: signal-handling regression test.**

    Add `tests/cmd_signal_passthrough_long_output.rs` (or extend the existing `tests/cmd_signal_observed.rs`):

    - New fixture `tests/fixtures/slow-streamer.sh`: prints `line-{n}` to stderr every 50 ms for 5 seconds, with `set -euo pipefail`.
    - Test starts `codex-session exec` with this fixture, lets it stream for ~250 ms, then sends SIGINT to the wrapper.
    - Assert: the captured `stderr` contains the lines printed before the kill (lines 1–~5), the wrapper exit code is `128 + SIGINT = 130`, and the lines are visible on the assertion's view of stderr (not buffered).

    This test would have caught B1 if it had existed in R4.

4.  **Fix I1: kebab-case JSON for `AccountCooldownEntryView`.**

    Single-line change in `src/commands/account/mod.rs` — replace `rename_all = "snake_case"` with `rename_all = "kebab-case"` on `AccountCooldownEntryView`:

    ```rust
    #[serde(rename_all = "kebab-case")]
    pub(crate) struct AccountCooldownEntryView { ... }
    ```

    Update `tests/account_cooldown_cli.rs` assertions:

    - `items[0]["cooled_down"]` → `items[0]["cooled-down"]`
    - `items[0]["reset_at_unix"]` (if asserted) → `items[0]["reset-at-unix"]`
    - `items[0]["last_429_at_unix"]` → `items[0]["last-429-at-unix"]`

    The on-disk `Cooldown` struct in `src/services/account/cooldown.rs` stays snake_case — that file is consumed only by `cooldown::read` / `cooldown::write` and is not part of the user-facing CLI JSON contract.

    Also update any snapshots under `tests/snapshots/cmd_account_help__account_cooldown_help.snap` if they include sample JSON output (they shouldn't, but verify before running `INSTA_UPDATE=always`).

5.  **Fix I2: route `cooldown::path` through `Registry`.**

    Change `cooldown::path` to accept either a `&Registry` or a precomputed `account_root: &Utf8Path` instead of `state_root: &Utf8Path`. The cleaner of the two surfaces:

    ```rust
    pub(crate) fn path(account_root: &Utf8Path) -> Utf8PathBuf {
        account_root.join("cooldown.json")
    }
    pub(crate) fn read(account_root: &Utf8Path) -> Result<Option<Cooldown>, CooldownError>;
    pub(crate) fn write(account_root: &Utf8Path, cooldown: &Cooldown) -> Result<(), CooldownError>;
    pub(crate) fn clear(account_root: &Utf8Path) -> Result<(), CooldownError>;
    ```

    `clear_all` becomes:

    ```rust
    pub(crate) fn clear_all(registry: &Registry) -> Result<usize, CooldownError>;
    ```

    so it iterates `registry.list()` (which already honors `registry_dir`) instead of `read_dir(state_root.join("accounts"))`.

    Update call sites:

    - `selector.rs::cooldown_active` — pass `registry.account_dir(account)` instead of `&ctx.config.paths.state_dir`.
    - `retry.rs::run_with_retry` — build a `Registry::from_config(&ctx.config)` once outside the loop, pass `registry.account_dir(&account)` to `cooldown::write`.
    - `commands/account/cooldown.rs::show` and `clear` — same, build a `Registry` and pass `account_dir(...)`.

    The `tests/account_cooldown_cli.rs::write_cooldown` helper at line 9-21 already writes via `env.named_account_root("work").join("cooldown.json")` which uses the **test's** state-root layout. Since the test never sets `registry_dir`, the helper stays correct. But verify by adding a focused unit test that exercises a custom `registry_dir` round-trip (see step 12.2 below).

6.  **Fix I3: drop module-scope `dead_code` allows.**

    - `src/services/account/failover.rs:16` — delete the line `#![allow(dead_code)]`. Run `just lint`; if clippy complains about anything, fix the specific item with a targeted `#[allow(dead_code, reason = "...")]`. Audit suggests nothing is currently dead, so this should be a no-op.
    - `src/services/account/cooldown.rs:1` — change to `#![allow(clippy::result_large_err)]` (drop `dead_code`). Make `path` private (`fn path(...)` instead of `pub(crate) fn path(...)`) since no external caller uses it; this lets the lint catch future drift.

7.  **Fix I4: collapse the double-pass detector.**

    Replace the body of `scan` in `src/services/account/failover.rs` with a single `RegexSet::matches` call:

    ```rust
    pub(crate) fn scan(buf: &[u8]) -> Option<Match> {
        for (idx, line) in buf.split(|byte| *byte == b'\n').enumerate() {
            let line = String::from_utf8_lossy(line);
            let matches = PATTERNS.matches(&line);
            if let Some(pattern_index) = matches.into_iter().next() {
                return Some(Match {
                    line_no: idx + 1,
                    snippet: truncate_for_debug(&line, 256),
                    pattern_index,
                });
            }
        }
        None
    }
    ```

    Extend `Match`:

    ```rust
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct Match {
        pub line_no: usize,
        pub snippet: String,
        pub pattern_index: usize,  // NEW: index into PATTERNS_RAW
    }
    ```

    Delete the `MATCHERS` static and the `truncate_for_debug` helper if no longer used (the inline `String::from_utf8_lossy(line)` does the truncation via `chars().take(256).collect::<String>()` in retry.rs already; consolidate if convenient).

    Update `retry.rs` to log `pattern_index = matched.pattern_index` in the `failover.match` structured-log record so operators can tell which pattern fired (useful for false-positive debugging).

    Add a test name lookup table in `failover.rs`:

    ```rust
    pub(crate) const PATTERN_NAMES: [&str; 6] = [
        "rate-limit", "quota-exceeded", "429", "too-many-requests",
        "exceeded-rate", "slow-down",
    ];
    ```

    Use this for log enrichment: `pattern = PATTERN_NAMES[matched.pattern_index]`.

    Update the inline tests so each assertion includes the expected `pattern_index` (e.g. `matches_http_429_too_many_requests` asserts `pattern_index == 0` because `rate-limit` comes first in the list and `429` and `too-many-requests` are at indices 2 and 3 — actually `HTTP 429 Too Many Requests` matches indices 2 (`\b429\b`) and 3 (`(?i)too.?many.?requests`); `RegexSet::matches.into_iter().next()` returns the lowest matching index, so the expected value is `2`). Document this ordering invariant in the doc comment.

8.  **Fix I5: rename `CooldownError::Parse`.**

    Split the variant into two and use them at the right call sites:

    ```rust
    #[derive(Debug, thiserror::Error)]
    pub(crate) enum CooldownError {
        #[error(transparent)]
        Fs(#[from] crate::adapters::fs::FsError),
        #[error("cooldown decode failed at {path}: {source}")]
        Decode {
            path: Utf8PathBuf,
            #[source] source: serde_json::Error,
        },
        #[error("cooldown encode failed at {path}: {source}")]
        Encode {
            path: Utf8PathBuf,
            #[source] source: serde_json::Error,
        },
        #[error("cooldown io failed at {path}: {source}")]
        Io {
            path: Utf8PathBuf,
            #[source] source: std::io::Error,
        },
    }
    ```

    Update:

    - `cooldown::read` → maps `serde_json::from_slice` failure to `Decode`.
    - `cooldown::write` → maps `serde_json::to_vec_pretty` failure to `Encode`.
    - `error::account_error_detail` and `AccountError::path()` → handle the new variants (mostly identical to the old `Parse` arm; just match both names).
    - `tests/` — no direct match on the variant name in existing tests, so no test churn expected.

9.  **Fix I6: pinned-account retry short-circuit + warning text.**

    Change `retry::run_with_retry`:

    ```rust
    pub(crate) fn run_with_retry(ctx: &AppContext, argv: &[OsString]) -> Result<i32, AppError> {
        let max_retries = ctx.global.max_retries;
        let is_pinned = matches!(
            ctx.global.account.as_ref(),
            Some(AccountSelector::Named(_)),
        );

        if max_retries > 0 && is_pinned {
            let name = match ctx.global.account.as_ref() {
                Some(AccountSelector::Named(n)) => n.clone(),
                _ => unreachable!(),
            };
            tracing::warn!(
                op = "account.pinned_retry_noop",
                account = %name,
                max_retries,
                "ignoring --max-retries with pinned --account: rotation requires --account auto"
            );
            ctx.ui.write_warning(
                "warning: --max-retries is ignored with pinned --account; retries with a pinned account would re-run the same account. Use --account auto for failover rotation.",
            )?;
            // Short-circuit: behave as if --max-retries=0 for the pinned path.
            return single_attempt(ctx, argv);
        }

        // ... existing retry loop unchanged ...
    }

    fn single_attempt(ctx: &AppContext, argv: &[OsString]) -> Result<i32, AppError> {
        let account = resolver::resolve(ctx)?.id;
        let (exit_code, _captured) = crate::commands::pass_through::run_once(ctx, argv, &account)?;
        Ok(exit_code)
    }
    ```

    Update `tests/account_failover_pinned.rs`:

    - **Before:** asserted 3 attempts (`--max-retries 2` → 3 invocations), cooldown.json written, all three attempts use the pinned account.
    - **After:** assert exactly 1 attempt (the short-circuit ran a single `pass_through::run_once`), exit code 1 (child's actual exit), warning text contains `"ignored"` (the new substring), and **no `cooldown.json` was written**. This is a deliberate behavior change.
    - Add a new positive assertion: stderr also contains the `--account auto` hint phrase so the user has a path forward.

    Add an inline unit test under `retry.rs` that exercises the short-circuit path via a mock `Spawner` and a constructed `AppContext` (if `tests/support/` already has helpers for this; otherwise keep the assertion in the integration test).

10. **Fix N1: harden the fixture.**

    Update `tests/fixtures/fake-429.sh`:

    ```bash
    #!/usr/bin/env bash
    set -euo pipefail
    {
      printf 'HTTP 429 Too Many Requests\n'
      printf 'marker:codex-session-fake-429 home=%s\n' "${CODEX_HOME:-unset}"
    } >&2
    exit 1
    ```

    Notice the new explicit `marker:` prefix (also addresses N3).

11. **Fix N2: log on `child_exit_code` truncation.**

    Change `src/commands/dispatch.rs:36-38`:

    ```rust
    fn child_exit_code(code: i32) -> u8 {
        u8::try_from(code).unwrap_or_else(|_| {
            tracing::warn!(
                op = "child.exit.clamped",
                original = code,
                clamped = u8::MAX,
                "child exit code does not fit in u8; clamping to 255",
            );
            u8::MAX
        })
    }
    ```

    No new tests needed; this is a debug-aid log only.

12. **Fix N3 + N4 + add I2 coverage.**

    Test updates:

    1. **N3:** update `tests/account_failover_retry.rs` and `tests/account_failover_pinned.rs` to parse `"marker:codex-session-fake-429 home="` instead of `"codex_home="`. The new prefix is explicit and survives any future env-var rename. (The pinned test is already being rewritten in step 9.)
    2. **N4:** add a new test in `tests/account_cooldown_cli.rs`:
        ```rust
        #[test]
        fn cooldown_clear_rejects_account_and_all_together() {
            let env = TestEnv::new();
            env.cmd()
                .args(["--account", "work", "account", "cooldown", "clear", "--all"])
                .assert()
                .failure()
                .code(64)
                .stderr(predicate::str::contains(
                    "--all conflicts with --account",
                ));
        }
        ```
        Update `src/commands/account/cooldown.rs::clear` to reject the combo before the `args.all` branch:
        ```rust
        if args.all && selected.is_some() {
            return Err(AppError::Usage(clap::Error::raw(
                clap::error::ErrorKind::ArgumentConflict,
                "--all conflicts with --account; pick one",
            )));
        }
        ```
    3. **I2 coverage:** add a new integration test `tests/account_cooldown_registry_dir.rs`:
        - Set up a `TestEnv` with `CODEX_SESSION_ACCOUNT_REGISTRY_DIR=$STATE/custom-accounts` (use `env.cmd_with_extra_env(...)` or extend the support helper).
        - `account add work`, then write a cooldown via `cooldown::write` (or run the wrapper to a 429).
        - Assert the cooldown.json appears at `$STATE/custom-accounts/work/cooldown.json`, NOT at `$STATE/accounts/work/cooldown.json`.
        - Run `account cooldown show --json` and assert it reads the custom path.
        - This test would have caught I2 if it had existed in R4.

13. **Update `.plan/multi-account-refactor/07-failover-spec.md`.**

    Reflect the post-R4.5 reality (both for any future plan-reviewer pass and for the next refactor round):

    - **Trigger surface section:** replace the invented `(?i)\b(429|rate[- ]limit|...)` alternation with the actual six-pattern caam Codex list:
        ```
        (?i)rate.?limit
        (?i)quota.?exceeded
        \b429\b
        (?i)too.?many.?requests
        (?i)exceeded.*rate
        (?i)slow.?down
        ```
        Cite `internal/ratelimit/detector.go` (`DefaultPatterns()[ProviderCodex]`) verbatim.
    - **Pattern test matrix:** flip the `rate_limit:` (JSON key) row from ✗ to ✅ — caam's `(?i)rate.?limit` matches it because `.` is a wildcard. Add a new ✗ row for `nominal capacity reached` (Codex list does not include `capacity`; the Claude list does — this locks in the per-provider choice).
    - **Detection placement section:** replace the entire "Tee strategy: post-wait scan, not mid-flight streaming" paragraph with the new tee strategy from step 2. Reference ADR D9 (added in step 14 below).
    - **Cooldown file schema:** update the path from `<state-root>/accounts/<account>/cooldown.json` to `<registry-root>/<account>/cooldown.json` where `<registry-root>` is `config.account.registry_dir` or `<state-root>/accounts` by default. Cite I2 fix.
    - **Retry-with-rotation harness section:** add a note that `--max-retries > 0` with pinned `--account` short-circuits to a single attempt (no rotation, no cooldown write). Cite I6 fix.

14. **Append `D9` ADR to `.plan/multi-account-refactor/04-decisions.md`.**

    New section at the end of the file, titled:

    > ## D9 — Tee-based stdio for `Spawner::spawn_and_wait_output`

    Document:

    - **Context:** R4 introduced `spawn_and_wait_output` with `Stdio::piped()` + `wait_with_output()` so `failover::scan` could see the child's bytes. This broke the interactive Codex TUI for every pass-through invocation (B1 in the R4 deep review).
    - **Decision:** the spawner runs two reader threads — one per pipe — that tee bytes into both the parent's live stdout/stderr AND a capture buffer. The captured buffer is what `failover::scan` reads. Live forwarding preserves the TUI experience; capture preserves the failover signal.
    - **Consequences:**
        - Interactive Codex sees real-time output again. TTY detection on the child is broken either way (the kernel-level fact is that the child's stdout is a pipe, not the parent's TTY); use the established Codex workaround (`CODEX_FORCE_TTY=1` or equivalent env) when interactive TUI mode is needed inside the wrapper. Note: this trade-off was unavoidable once we needed to scan child output for failover detection; the alternative (no scan) would defeat R4.
        - Memory: the capture buffer is capped at 1 MiB by `failover::MAX_CAPTURE_BYTES`; the live forwarding is unbounded but streamed (no growth on the wrapper side). Documented in `07-failover-spec.md`.
    - **Alternatives considered:**
        - **Fast-path bypass for `max_retries == 0`:** keep the old inherited-stdio `spawn_and_wait` for the common path and only switch to piped when failover is opted in. Rejected: the new behavior should be consistent across modes so future selector improvements don't bifurcate the spawn path.
        - **Add `adapters/spawner.rs` to the print-ownership lint whitelist:** rejected. Adding a new module to the whitelist erodes a load-bearing invariant. The tee helper lives in `src/ui/raw_passthrough.rs` instead.
    - **References:** R4.5 prex input (this file's step 2), `src/adapters/spawner.rs::spawn_and_wait_output` post-fix, `src/ui/raw_passthrough.rs::tee_to_stdio`.

15. **Update `.plan/multi-account-refactor/99-execution-plan.md`.**

    Mark R4 as `✅ Done (hardened by R4.5)`, insert R4.5 as its own row, bump R5's link to `15-phase-skills-reintegration.md`. See step 9 of the previous round's plan for how the existing rows are formatted; mirror that style exactly.

## Files touched (representative)

- `src/adapters/spawner.rs` (rewrite `spawn_and_wait_output` body for tee strategy)
- `src/ui/raw_passthrough.rs` (add `tee_to_stdio`; possibly drop `#![allow(dead_code)]`)
- `src/commands/pass_through.rs` (delete the post-wait `write_raw` calls)
- `src/commands/dispatch.rs` (log on truncation)
- `src/commands/account/mod.rs` (kebab-case rename)
- `src/commands/account/cooldown.rs` (reject `--all + --account`, route via Registry)
- `src/services/account/failover.rs` (drop module dead_code, collapse double-pass, extend `Match` with `pattern_index`, add `PATTERN_NAMES`, add `MAX_CAPTURE_BYTES`)
- `src/services/account/cooldown.rs` (drop module dead_code, switch to `account_root`-based API, rename `Parse` → `Decode`/`Encode`)
- `src/services/account/selector.rs` (use `registry.account_dir(...)` for cooldown reads)
- `src/services/account/retry.rs` (pinned short-circuit, registry-aware cooldown writes, log pattern name + index)
- `src/services/account/error.rs` (handle new `CooldownError::Decode`/`Encode` variants in `kind()` and `path()`)
- `src/error.rs` (handle new cooldown error variants in `account_error_detail`)
- `tests/fixtures/fake-429.sh` (set -euo pipefail, marker: prefix)
- `tests/fixtures/slow-streamer.sh` (NEW — for SIGINT regression)
- `tests/account_cooldown_cli.rs` (kebab-case JSON assertions, --all/--account conflict test)
- `tests/account_cooldown_registry_dir.rs` (NEW — I2 regression coverage)
- `tests/account_failover_pinned.rs` (rewrite around short-circuit semantics + marker: prefix)
- `tests/account_failover_retry.rs` (marker: prefix)
- `tests/cmd_signal_passthrough_long_output.rs` (NEW — Q1 + B1 regression coverage)
- `.plan/multi-account-refactor/07-failover-spec.md` (regex matrix, tee strategy, registry-aware path)
- `.plan/multi-account-refactor/04-decisions.md` (append D9)
- `.plan/multi-account-refactor/99-execution-plan.md` (R4 done, R4.5 row inserted)

**Net LOC estimate:** ~250–350 (incl. ~100 LOC test additions, ~50 LOC doc updates). **New tests:** 4 files + 2 inline.

## Done criteria

```sh
just precommit-all     # must exit 0
```

Plus the manual smoke list — same as R4's done criteria, but with the tee-and-interactive validation added:

```sh
# Interactive Codex TUI works again. Run a bare `codex-session` invocation in a terminal,
# verify that prompts appear immediately, color is preserved, and Ctrl-C terminates cleanly.
codex-session

# Fixture-driven 429 with --account auto: same expectation as R4 (rotates, exits 75 if all fail).
CODEX_SESSION_CHILD_BIN=$(realpath tests/fixtures/fake-429.sh) \
  codex-session --account auto --max-retries 2 exec "trigger 429"

# Pinned + --max-retries: NEW behavior — short-circuit, single attempt, warning printed,
# no cooldown file written.
codex-session --account work --max-retries 2 exec "hi" 2>&1 | grep "ignored"
ls ~/.local/state/codex-session/accounts/work/cooldown.json   # must NOT exist

# Custom registry_dir: cooldown writes/reads honor the override.
CODEX_SESSION_ACCOUNT_REGISTRY_DIR=/tmp/custom-accounts \
  codex-session --account auto --max-retries 1 exec "trigger 429" || true
ls /tmp/custom-accounts/*/cooldown.json   # must exist
ls ~/.local/state/codex-session/accounts/*/cooldown.json 2>/dev/null   # must NOT exist for any account

# JSON schema is kebab-case.
codex-session account cooldown show --json | jq 'first | has("cooled-down")'   # → true
codex-session account cooldown show --json | jq 'first | has("cooled_down")'   # → false

# --all conflicts with --account (N4).
codex-session --account work account cooldown clear --all 2>&1 | grep "conflicts with"

# SIGINT mid-stream preserves output (Q1).
( codex-session exec 'long_streaming_task' & PID=$!; sleep 0.5; kill -INT $PID; wait $PID )
# Expected: lines printed before the SIGINT are visible on the terminal,
# wrapper exits 130, no panic.
```

## Out of scope for Round 4.5

- Mid-session rotation (loopback proxy à la `ndycode`) — same as R4.
- Streaming detection during child execution (R4.5's tee still scans **after** wait, on the captured buffer; streaming detection would require a different design and is not needed for the `codex exec` workload).
- Per-account telemetry / TUI dashboard.
- Per-model quota tracking.
- Honoring `Retry-After:` headers parsed from captured output.
- Re-enabling TTY for the child despite the pipe (would require a PTY, out of scope).
- Reverting any of R4's choices (the six-pattern caam list, the `--max-retries` flag, the AuthBridge deletion). Those stand.

## Constraints

- Use `just` recipes for verification, not raw `cargo`.
- The tee helper must live in `src/ui/raw_passthrough.rs` to keep the print-ownership lint whitelist intact. Do NOT add `adapters/` to the lint whitelist.
- `MAX_CAPTURE_BYTES = 1 << 20` (1 MiB) is the cap. Document with a `// see ADR D9` comment.
- `tracing::warn!` for `child.exit.clamped` and `account.pinned_retry_noop` use the project's `op=` structured-log convention.
- The `--all conflicts with --account` error must surface as `AppError::Usage` so it exits 64 (already covered by the existing usage-mapping in `error.rs:138`).
- No `unsafe` blocks. The tee threads use only `std::io` primitives.
- The pinned-account short-circuit must not silently swallow the user's `--max-retries N` — log it (already in step 9's `tracing::warn!`) so the behavior change is observable in the wrapper log.
- The `tests/account_failover_pinned.rs` test gets rewritten, not deleted; preserve the existing positive assertion that the warning text appears.
- Keep the AuthBridge deletion (from R4) intact. R4.5 does NOT touch `src/services/auth.rs` or `src/services/auth_inspect.rs`.
- Snapshot updates (`tests/snapshots/cmd_account_help__*`, root-help) should be regenerated only if the help text legitimately changed. The kebab-case JSON rename does not touch any --help output; running `insta review` should yield zero diff for those.

## Risk register

| Risk | Likelihood | Mitigation |
|---|---|---|
| Tee threads deadlock if child writes huge bursts and the parent's stdout is itself piped to a slow consumer | Low | Use 8 KiB read buffer, no internal queue; let the pipe backpressure naturally propagate. |
| `child.wait()` returns before the tee threads finish draining | Medium | Join the threads **after** `wait()` returns; the kernel keeps pipe contents readable even after the writer dies, so the threads see EOF and return cleanly. |
| `tee_to_stdio` panics on broken pipe (parent stdout closed) | Medium | Match on `io::ErrorKind::BrokenPipe` and treat as "stop forwarding, keep capturing". Use `let _ = sink.write_all(...)` for the live half; propagate other I/O errors. |
| `Match::pattern_index` change is a breaking API change for downstream callers of `failover::scan` | Low | The function is `pub(crate)`; only `retry.rs` calls it. Update that one site. |
| Registry-aware cooldown path breaks the existing `tests/account_cooldown_cli.rs::write_cooldown` helper | Low | The helper writes via `env.named_account_root("work").join("cooldown.json")` which still resolves to the default registry layout. The new I2-coverage test sets a custom `registry_dir` explicitly. |
| `--max-retries N` env mirror is added accidentally during the I6 rewrite | Low | Keep CLI-only (Q3 of the R4 plan-review was deferred). The `#[arg]` attribute does NOT gain `env = "CODEX_SESSION_MAX_RETRIES"`. |
| Q1 regression test is flaky on slow CI runners | Medium | Use 50 ms inter-line delay (long enough to be racy if buffered, short enough that 5 s budget is generous). Pin `RUST_TEST_THREADS=1` in the test attrs if needed. |
