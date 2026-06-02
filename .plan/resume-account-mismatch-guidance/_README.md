# Resume Account-Mismatch: Steer the Right Account & Command

> Complexity: M | Rounds: 1 | Generated: 2026-06-02 | Repo: /workspaces/codex-session

## Problem Statement

`codex-session exec resume <ID>` resolves a thread's owning account from an on-disk thread-index.
On an index **miss**, the wrapper silently falls back to account auto-selection and forwards the
resume to whatever it picks. Because codex rollouts are scoped per-account
(`accounts/<acct>/groups/<g>/sessions/…/rollout-*.jsonl`), the picked account almost never owns the
rollout, so codex fails with an opaque `thread/resume failed: no rollout found … (code -32600)`.

This burned a real `prex` run: stage-1 planning created the thread under account **P**; stage-3
resume missed the index, auto-selected another account, and got `-32600`. `docs/upstream-codex.md`
F16 already warns: the wrapper "must keep the resume pinned to the index entry's account."

The fix makes the wrapper **diagnose and steer** instead of leaking `-32600`, and updates the `prex`
skill to cooperate with the clearer signals.

## Strategy

One cohesive change to the resume path plus a paired `prex`-skill update. On an index miss the
wrapper **discovers the true owner** by scanning each account's rollout store and **auto-pins +
warns** (resume just works); if no account owns it, it emits a rich classified error with recent-thread
candidates and the fresh-`exec` command; and a post-exec `-32600` (sandbox mismatch / deleted
rollout) is classified rather than leaked. The `prex` skill then pins the planning account into the
stage-3 resume and reworks its fallback decision table to match. Kept as **one round** (per user
decision; consistent with the M grade) — the wrapper work is one Codex implement session and the
`prex` edit is a discrete final step.

This plan executes **after** `codex-error-classification-cooldown`, reusing its general
unhandled-error verbose+log handler and event/text classification scaffolding.

## Rounds

Authoritative order/status live in `_QUEUE.yaml`; this list mirrors it.

1. `wrapper-guidance-and-prex-sync.md` — owner discovery + auto-pin/warn, `ResumeOwnerMissing` error
   with candidates, post-exec `-32600` classification, `upstream-codex.md` update, tests, and the
   `prex` SKILL.md pin + fallback-table rewrite.

## Execution Commands

```bash
# Execute the next todo round (executor reads _QUEUE.yaml, runs the first `todo` round, then stops):
/prex -ar @.plan/resume-account-mismatch-guidance/

# Or target the round file directly:
/prex -ar .plan/resume-account-mismatch-guidance/wrapper-guidance-and-prex-sync.md
```

## Execution Discipline

**Rounds must be executed one at a time.** Each round is a self-contained unit of work designed for a
single `/prex` session. Do not implement multiple rounds in one session.

When `/prex` is pointed at this directory or this `_README.md`, it MUST:

1. Read this plan's `_QUEUE.yaml`.
2. Find the first round with status `todo`.
3. Execute ONLY that round, then stop.
4. End the session — a fresh `/prex` session is launched for any subsequent round.

**Why:** Fresh sessions prevent context contamination, keep token usage predictable, and let the user
review intermediate results before proceeding. (This plan has a single round, but the discipline
still applies: do not also start a dependent plan in the same session.)

## Decisions & Constraints

- **Executor: prex (EF 1.5).**
- **Depends on `codex-error-classification-cooldown` being `done`.** This round reuses that plan's
  general unhandled-error handler (styled stderr + tracing log) and event/text classification for the
  post-exec `-32600` step. If a helper is absent at execution time, implement a minimal local
  equivalent rather than blocking.
- **Index miss with a discoverable owner → auto-pin + loud `warning:`** (user decision). The resume
  re-pins to the owner found on disk and succeeds; a `BOLD_YELLOW` warning names the owner and the
  index gap. Backfill the index so the next resume is a clean hit.
- **Thread owned by no account → rich diagnostic** (user decision): a classified `ResumeOwnerMissing`
  error that distinguishes typo vs. lost index, lists recent known threads as candidates, and gives
  the fresh-`exec` command. Never auto-select and forward.
- **Classify the post-exec `-32600` too** (user decision): a correctly-pinned resume that still gets
  `-32600` is mapped to sandbox-mismatch (F15 → re-run with same `--sandbox`) vs. deleted-rollout
  (start fresh), not leaked raw.
- **Single combined round** (user decision). Formula: files:3 cross-cut:3 deps:2 novelty:3 risk:3 →
  raw 14 ÷ EF 1.5 → 9.33 → **M (1 round)**; the user confirmed keeping the wrapper fix and the `prex`
  update in one round.
- **No cross-account resume.** Stock codex cannot resume a thread under a non-owning account (F16);
  the wrapper only ever pins to the owner — it never migrates a rollout.
- **CLI output obeys `docs/design/cli-style-guide.md`:** stderr ownership, `BOLD_RED` error /
  `BOLD_CYAN` hint / `BOLD_YELLOW` warning; never restyle passthrough child output. Wrapper
  machine-readable output uses `--format json`, never `--json`.
- **Quality gates via `just`** (`just lint`, `just test-unit`, `just test-integration`), not raw cargo.

## Rejected Alternatives

- **Keep the silent auto-select fallback, just log it** — rejected: it produces the opaque `-32600`
  that wasted the run; the caller gets no steer.
- **Auto-pin silently (no warning)** — rejected by the user: hides the index gap from operators.
- **Refuse-and-error even when the owner is discoverable** — considered; rejected in favor of
  auto-pin + warning for better agent/human ergonomics (the resume just works).
- **Split into 2–3 rounds (wrapper vs. prex, or finer)** — offered; user chose a single combined
  round, consistent with the M grade.

## Risks & Edge Cases

- **Cross-repo edit (Step 7).** The `prex` skill lives in `/home/gu/.dotfiles`, outside this repo, so
  a single `/prex` session that edits both trees needs its sandbox/cwd scoped to make `~/.dotfiles`
  writable. Mitigation: run the implementation with cwd `~/.dotfiles` for that step, or apply the one
  SKILL.md edit by hand. Needs handling.
- **Combined-round size.** Owner discovery + auto-pin + candidates + post-exec classification + tests
  - the cross-repo skill edit may strain one 600s Codex implement window. Mitigation: the `prex` edit
    is the last, separable step — if the session runs long, land the wrapper change (Steps 1–6) and do
    Step 7 separately. The prex review-loop further de-risks size.
- **Rollout filename vs. content match.** If codex's rollout filename does not embed the thread uuid
  in the expected form, the cheap filename scan misses; fall back to a bounded content scan. Accepted;
  covered by the discovery unit test.
- **Misclassifying `-32600`.** Sandbox-mismatch vs. deleted-rollout can be ambiguous; bias the hint
  toward the safe, reversible action (re-run with same sandbox) and always log the full snippet.

## Completion

When the round is done, set it `done` in this plan's `_QUEUE.yaml` and set this plan `done` in the
top-level `.plan/_QUEUE.yaml`. Nothing moves on disk.
