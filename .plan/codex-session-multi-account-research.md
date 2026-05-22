# Codex Session — Multi-Account Auth Research [DEPRECATED]

**Status:** Superseded 2026-05-22 — content consolidated into [`multi-account-refactor/`](multi-account-refactor/).
**Original last updated:** 2026-05-18.

This file's content has been split (DRY) into the multi-file refactor plan. Look for it here:

| Original section | New home |
|---|---|
| §1 Goal | [`multi-account-refactor/00-overview.md`](multi-account-refactor/00-overview.md) |
| §2 Current state | [`multi-account-refactor/01-architecture.md`](multi-account-refactor/01-architecture.md) |
| §3 Native multi-account support? | [`multi-account-refactor/02-references.md`](multi-account-refactor/02-references.md) F4 |
| §4 Reading remaining quota out-of-band | [`multi-account-refactor/02-references.md`](multi-account-refactor/02-references.md) F5 + [`06-quota-protocol.md`](multi-account-refactor/06-quota-protocol.md) |
| §5 Rotating `~/.codex/auth.json` feasibility | [`multi-account-refactor/02-references.md`](multi-account-refactor/02-references.md) F6 + ADR [D3](multi-account-refactor/04-decisions.md) |
| §6 Existing projects (inventory) | [`multi-account-refactor/03-inspired-projects.md`](multi-account-refactor/03-inspired-projects.md) |
| §7 Recommended architecture | [`multi-account-refactor/01-architecture.md`](multi-account-refactor/01-architecture.md) + [`04-decisions.md`](multi-account-refactor/04-decisions.md) + [`05-cli-design.md`](multi-account-refactor/05-cli-design.md) |
| §8 Ranking summary | [`multi-account-refactor/03-inspired-projects.md`](multi-account-refactor/03-inspired-projects.md) "Summary table" |
| §9 Open questions | Resolved or deferred — see [`04-decisions.md`](multi-account-refactor/04-decisions.md) and [`99-execution-plan.md`](multi-account-refactor/99-execution-plan.md) "Post-merge follow-ups" |
| §10 References | [`multi-account-refactor/02-references.md`](multi-account-refactor/02-references.md) "External references (master list)" |

## Why this was deprecated

The original 302-line research file was a useful one-shot inventory but mixed multiple concerns (upstream codex facts, project comparisons, architectural recommendation, open questions). The execution plan needs each concern as a focused, navigable file:

- `00-overview.md` for the north star.
- `01-architecture.md` for layout / call-flow.
- `02-references.md` for the upstream codex SoT (cross-referenced with `docs/upstream-codex.md`).
- `03-inspired-projects.md` for the per-project comparison + what-we-borrowed table.
- `04-decisions.md` for ADRs (each load-bearing decision separately citable).
- `05-cli-design.md` for the CLI surface design + conformance against `tech/programming/cli-design/`.
- `06-quota-protocol.md` for the `wham/usage` endpoint spec.
- `07-failover-spec.md` for the regex 429 detector + cooldown schema.
- `10-13-phase-*.md` for each prex-round task description.
- `99-execution-plan.md` for difficulty / sequencing / verification.

The full implementation plan (entry point) is at the dir's [README candidate / 00-overview.md](multi-account-refactor/00-overview.md). The single-file master plan is at `/home/gu/.local/state/claude-session/sessions/pts-0/plans/recursive-hatching-whale.md` (session-local, not in git).
