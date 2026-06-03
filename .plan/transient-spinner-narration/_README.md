# Transient Spinner Narration — no leftover ✓/✗ lines above reports

> Complexity: S | Rounds: 1 | Generated: 2026-06-03 | Repo: /workspaces/codex-session

## Problem Statement

`codex-session account quota` leaves persistent `✓ cwnt` / `✓ isma` / `✓ mari` lines on stderr
above the quota report:

```text
✓ cwnt
✓ isma
✓ mari                                                                   #1 -946.00 cwnt
  5-hour      ░░░░░░░░░░░░░░░░░░░░  0% left   resets in 1h 22m
  ...
```

These come from `SpinnerHandle::finish_ok(account)` in the per-account fetch tasks — each
spinner converts into a durable completion-marker line instead of clearing. Spinner narration
should be **pure wait-time feedback**: spinners (and transient checkmarks while other accounts
are still fetching) are fine, but once work completes everything must wipe so only the clean
report remains.

A review of all spinner consumers found the same leftover pattern in five commands:

| Command                           | Leftover today                                    | Redundant with                                                  |
| --------------------------------- | ------------------------------------------------- | --------------------------------------------------------------- |
| `account quota` (multi)           | `✓ <name>` / `✗ <name> — err` per account         | report shows each account incl. inline red `Error: ...` entries |
| `account quota --account X`       | `✓ X` or `✗ X — err`                              | report / error renderer prints the same info                    |
| `account health`                  | `✓ <name>` / `✗ <name> — status` per account      | report table shows status/token per account                     |
| `doctor`                          | `✓ All checks passed` / `✗ N checks failed`       | report prints per-check symbols + `summary:` line               |
| `account add` / `account refresh` | `✓ Account "X" added` / `✓ Credentials refreshed` | stdout mutation block `✓ account added: X` follows immediately  |

## Strategy

Single round: keep per-account `finish_ok`/`finish_err` markers as _transient_ live feedback in
the multi-spinner groups (quota multi, health), then wipe the whole group via
`SpinnerGroup::clear()` once the last task finishes — before the report prints. Single-spinner
commands (doctor, add, refresh, single-account quota) switch to `finish_and_clear()`. Update
the CLI style guide §9b so the transient-only rule is the documented contract.

## Rounds

1. `clear-spinner-leftovers.md` — wipe spinner groups before reports; clear single spinners;
   update style guide §9b; remove dead `DoctorFinish` plumbing.

## Execution Commands

```bash
# Execute the next todo round (executor reads _QUEUE.yaml, runs the first `todo` round, then stops):
/prex -ar @.plan/transient-spinner-narration/

# Or target the round file directly:
/prex -ar .plan/transient-spinner-narration/clear-spinner-leftovers.md
```

## Execution Discipline

**Rounds must be executed one at a time.** Each round is a self-contained unit of work designed
for a single `/prex` session. Do not implement multiple rounds in one session.

When `/prex` is pointed at this directory or this `_README.md`, it MUST:

1. Read this plan's `_QUEUE.yaml`.
2. Find the first round with status `todo`.
3. Execute ONLY that round, then stop.
4. End the session — a fresh `/prex` session is launched for any subsequent round.

Why: fresh sessions prevent context contamination between rounds, keep token usage predictable,
and allow the user to review intermediate results before proceeding.

## Decisions & Constraints

- Executor: prex (EF 1.5).
- **Clear immediately when the last task finishes** — no artificial "blink hold" delay.
  Accounts that finish early still show their transient ✓ while slower accounts fetch; once the
  last one completes the whole group wipes and the report prints. (User-confirmed.)
- **Wipe ✗ error lines too** — spinner narration is fully transient; the report / error
  renderer owns all durable output, including errors. The quota report renders per-account
  inline `Error: ...` entries and the health report shows per-account status, so nothing is
  lost. (User-confirmed.)
- `finish_ok` / `finish_err` and their message helpers stay — they still render the transient
  in-group markers for quota-multi and health. Their unit tests stay.
- Pre-commit hooks are the quality gates: verify with `just lint`, `just test-unit`,
  `just test-integration` — not raw `cargo`.
- Style guide (`docs/design/cli-style-guide.md` §9b) is the source of truth for spinner
  behavior and must be updated in the same round.

## Rejected Alternatives

- **Hold ~300ms after the last finish so final checkmarks visibly "blink"** — rejected by user;
  adds fixed latency to every invocation for cosmetic effect.
- **Keep ✗ failure lines persistent above the report** — rejected by user; duplicates the
  report's inline error entries.
- **Per-handle `finish_and_clear()` on success in the multi-account paths** — rejected: each
  spinner line would vanish the instant its account completes, so the user gets no per-account
  completion feedback at all while slower accounts still fetch. Group-level clear preserves the
  transient ✓/✗ markers until all work is done.

## Risks & Edge Cases

- **indicatif `MultiProgress::clear()` vs finished bars** (indicatif 0.18): `clear()` wipes the
  whole drawn region including lines of finished-but-not-removed bars; all bars are finished
  before `clear()` is called (the join loop has completed) so nothing redraws afterwards. Needs
  a manual TTY verification (accepted; covered in acceptance criteria).
- **Integration tests** run with piped (non-TTY) stderr, so spinners are already hidden there —
  no integration-test changes expected. `tests/cmd_doctor_spinner.rs` assertions (no frames /
  ANSI on pipes) still hold.
- **Dead code after the doctor change**: `DoctorFinish` / `doctor_finish()` and their three
  unit tests become unused and must be removed or `clippy-strict` fails the gate.

## Completion

When all rounds are done, set each round `done` in this plan's `_QUEUE.yaml` and set this plan
`done` in the top-level `.plan/_QUEUE.yaml`. Nothing moves on disk.
