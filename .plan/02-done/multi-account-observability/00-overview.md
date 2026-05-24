# 00 — Overview

## North star

Complete the multi-account feature's **observability and diagnostics** surface.
The feature itself (R1–R5) is fully implemented and working. What's missing is
user-facing introspection: `doctor`, `--dry-run`, `config status`, `version`,
session metadata, logging, help text, safety warnings on destructive operations,
and documentation. Users should be able to see, understand, and debug what the
multi-account system is doing without reading source code.

## Why now

The R1–R5 refactor delivered working multi-account support, but the diagnostic
commands were not updated to expose the new state they now depend on:

- `doctor` reports `has_auth=true` but not cooldown status, auth freshness, or
  actionable next steps when auth is missing.
- `--dry-run` shows `CODEX_HOME` but not which account was selected or how.
- `config status` shows the resolved account name but nothing about auth health
  or how many accounts exist.
- `session-meta.json` doesn't record which account was used — post-hoc debugging
  loses this context.
- `account remove` silently archives the active account with no warning.
- `version` doesn't show the resolved account.
- `help_extras.txt` lists account subcommands but omits `cooldown`.
- README.md and DEVELOPMENT.md have zero mention of the multi-account feature.

## What this plan delivers

1. **Doctor enrichment (R6):** Per-account cooldown status in the accounts
    list, active-account auth check, `hint_for()` coverage for auth/account
    checks, WARN-level actionable next steps.
2. **Dry-run account context (R6):** Account name and resolution source in the
    `--dry-run` report.
3. **Session metadata (R6):** Account and account_source fields in
    `session-meta.json`.
4. **Help text fix (R6):** `cooldown` added to the account subcommand list.
5. **Config status enrichment (R7):** Auth health summary, account count,
    cooldown count.
6. **Version enrichment (R7):** Resolved account in `version` output.
7. **Safety warnings (R7):** `account remove` warns when removing the active
    account or one with recent sessions.
8. **Logging (R7):** Account context in retry tracing spans.
9. **Documentation (R7):** README.md and DEVELOPMENT.md updated with
    multi-account feature coverage.

## What this plan does NOT do (non-goals)

- Live API key validation (test call to verify key works). File-health checks
  are sufficient for diagnostics; runtime failures are self-evident.
- Interactive confirmation prompts on `account remove`. Warnings are printed;
  the command proceeds. A `--force` flag to suppress warnings is a future
  enhancement.
- Per-account quota display in `doctor`. The `account quota` subcommand already
  serves this purpose; duplicating it in doctor would add network calls to a
  command that should be fast and offline.

## Execution

| # | Name | Plan input | Difficulty | Est duration | Status |
|---|---|---|---|---|---|
| R6 | Doctor diagnostics + dry-run account context | [16-phase-doctor-dryrun.md](16-phase-doctor-dryrun.md) | Medium | 25–35 min | Pending |
| R7 | Config status + logging + safety + docs | [17-phase-config-safety-docs.md](17-phase-config-safety-docs.md) | Medium | 25–35 min | Pending |

R6 must land before R7 (hard dependency — R7's config status reads the same
cooldown primitives that R6 validates, and R7's README references the help text
that R6 fixes).

## Where to read next

- [16-phase-doctor-dryrun.md](16-phase-doctor-dryrun.md) — Round 6 prex input.
- [17-phase-config-safety-docs.md](17-phase-config-safety-docs.md) — Round 7 prex input.
- [../multi-account-refactor/00-overview.md](../multi-account-refactor/00-overview.md) — the original R1–R5 plan.
