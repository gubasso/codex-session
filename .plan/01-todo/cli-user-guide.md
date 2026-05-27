# Plan: CLI user guide at `docs/`

## Goal

Create a multi-file user-facing guide under `docs/guide/` that explains every
`codex-session` CLI flag and subcommand the way a knowledgeable colleague would
explain them to a new user — with rationale, resolution chains, validation
rules, practical examples, and "what happens when" scenarios.

The README stays as-is (terse reference). The guide is the place a new user
reads *first* to understand the tool.

## Audience

Someone who has installed `codex-session` and run `codex-session --help` but
doesn't yet understand what `--group`, `--account auto`, `--max-retries`,
`--config-recipe`, `--dry-run`, etc. actually *do* under the hood.

## Deliverables

### 1. `docs/guide/README.md` — index / landing page

Short intro (what `codex-session` is, how it wraps `codex`), then a linked
table of contents pointing to the topic files below. Keeps the guide
navigable as it grows.

### 2. `docs/guide/global-flags.md` — wrapper-owned global flags

One section per flag, each covering:

| Flag | Key topics to document |
|---|---|
| `--group <ID>` | What a group-id is, why it exists, session-dir scoping, the 5-step resolution chain (flag → env → tty → ppid → pid) with source table, validation rules (1–32 chars, `[a-z0-9][a-z0-9_-]*`), pid-N fallback warning, practical consequences of shared vs. isolated groups, examples. Source: `src/services/session/group_id.rs`. |
| `--account <NAME\|auto>` | What an account is, `auto` vs. named, the 7-step resolution chain (flag → env(auto/named) → lru → config-pinned → default → fallback), how `auto` triggers the quota-aware selector, `CODEX_SESSION_ACCOUNT` env override. Source: `src/services/account/resolver.rs`. |
| `--max-retries <N>` | What is retried (the entire child invocation), requires `--account auto`, total attempts = N+1, 429 pattern detection (6 patterns from `failover.rs`), 300s cooldown write, exit code 75 on exhaustion, warning when used without `auto`. Source: `src/services/account/retry.rs`, `failover.rs`. |
| `--config-recipe <NAME>` | What a config_recipe is, how settings-layers are composed, `CODEX_SESSION_CONFIG_RECIPE` env override. Source: `src/cli/config_recipe.rs`, config_recipe compose logic. |
| `--dry-run` | Prints resolved invocation and exits, useful for debugging session-dir and env composition. |
| `-v` / `--verbose` | Verbosity levels: `-v` info, `-vv` debug, `-vvv` trace. Goes to log file and optionally stderr. |
| `--log-stderr` | Mirror wrapper logs to stderr. |
| `-q` / `--quiet` | Suppress non-error stderr; log file unaffected. |
| `--silent` | Suppress all stderr including errors; log file unaffected. Conflicts with `--quiet`. |
| `--log-format <FMT>` | Controls stderr mirror format; file sink is always JSON. |
| `-V` / `--version` | Print wrapper + child version. |
| `--format <text\|json>` | Output format for `version`, `config status`, `config_recipe list/show`. |
| `--config <PATH>` | Load wrapper config from explicit path instead of default XDG locations. |

### 3. `docs/guide/subcommands.md` — wrapper-owned subcommands

One section per subcommand:

| Subcommand | Key topics |
|---|---|
| `version` | `--format` option, what "wrapper + child" means. |
| `completion <SHELL>` | Supported shells, how to install. |
| `config status` | What it reports, `--format` option. |
| `config_recipe list` | Lists available profiles, `--format` option. |
| `config_recipe show [NAME]` | Shows manifest and resolved layers, `--format` option. |
| `config_recipe compose [NAME]` | Composes into current session-dir. |
| `doctor` | Health checks, `--all-config-recipes`, `--show-env` (redaction rules). |
| `account add <NAME>` | Registers account, `--from-native` seeds from `~/.codex/auth.json`, name validation. |
| `account list` | Lists registered accounts, `--format`. |
| `account current` | Prints active account and its resolution source, `--format`. |
| `account use <NAME>` | Pins active account via `state/last-account`. |
| `account remove <NAME>` | Archives to `.trash/`. |
| `account quota` | `--live`, `--all`, `--format`. Reads cached or live wham/usage endpoint. |
| `account cooldown show\|clear` | Shows/clears failover cooldown state. `--all`, `--json`. |

### 4. `docs/guide/session-model.md` — session directory model

Explains the conceptual model that ties the flags together:

- What a "session" is in `codex-session`.
- The directory hierarchy: `<root>/accounts/<account>/groups/<group-id>/`.
- How `CODEX_HOME` is exported.
- How profiles compose settings into the session-dir.
- What files live in a session-dir (`config.toml`, `.codex-session-compose.json`, `session-meta.json`).
- Diagram of the full path from CLI invocation → flag resolution → session-dir → child env.

### 5. `docs/guide/multi-account.md` — multi-account & failover

Deep dive into the account system:

- Registering and managing accounts.
- Account resolution priority chain (7 steps, with table).
- Auto-selection and quota-aware picking.
- Failover: how `--max-retries` + `--account auto` work together.
- 429 detection patterns (the 6 regexes from `failover.rs`).
- Cooldown mechanics (300s, per-account `cooldown.json`).
- Exit code 75 (all accounts exhausted).
- Practical scenarios: single account, two accounts with failover, quota monitoring.

### 6. `docs/guide/environment-variables.md` — env var reference

Comprehensive table of all `CODEX_SESSION_*` env vars with:

- Name, purpose, example value.
- Which flag it corresponds to (if any).
- Note that `CODEX_SESSION_*` vars are scrubbed from the child environment.
- Non-`CODEX_SESSION_*` vars honored: `NO_COLOR`, `FORCE_COLOR`, `CLICOLOR`, `CLICOLOR_FORCE`, `RUST_LOG`.

## Approach

### Source of truth

Each doc section is written by reading the Rust source (clap definitions,
resolution functions, validation functions, tests) — not by paraphrasing
the README. Cross-reference actual code to ensure accuracy.

### Key source files

| File | What it documents |
|---|---|
| `src/cli/mod.rs` | All global flags (clap `GlobalArgs` struct) |
| `src/cli/account.rs` | Account subcommand tree |
| `src/cli/config_recipe.rs` | ConfigRecipe subcommand tree |
| `src/cli/doctor.rs` | Doctor flags |
| `src/cli/config.rs` | Config subcommand tree |
| `src/cli/completion.rs` | Completion args |
| `src/cli/version.rs` | Version args |
| `src/services/session/group_id.rs` | Group-id resolution chain + validation |
| `src/services/account/resolver.rs` | Account resolution chain |
| `src/services/account/retry.rs` | Retry/failover loop |
| `src/services/account/failover.rs` | 429 pattern detection |
| `src/services/account/cooldown.rs` | Cooldown read/write |
| `src/services/session/dir.rs` | Session directory creation |
| `src/ui/help_extras.txt` | Built-in extended help text |

### Style

- Conversational but precise — explain *why* things work the way they do, not just *what* they do.
- Include resolution priority tables with numbered steps and source labels.
- Include validation rule tables.
- Use concrete examples (`codex-session --group stable exec "hi"`) throughout.
- Use "What happens when..." scenarios to build intuition.
- Keep each file focused on one topic; cross-link between files.

### What NOT to change

- Do not modify `README.md` — it stays as the terse reference it is.
- Do not modify any Rust source files.
- Do not modify `CLAUDE.md`.

## File tree after implementation

```
docs/
  upstream-codex.md          (existing, untouched)
  guide/
    README.md                (index)
    global-flags.md          (all global flags explained)
    subcommands.md           (all wrapper subcommands explained)
    session-model.md         (session directory conceptual model)
    multi-account.md         (accounts, failover, cooldowns)
    environment-variables.md (env var reference)
```

## Order of implementation

1. `docs/guide/session-model.md` — foundational concepts needed by all other pages.
2. `docs/guide/global-flags.md` — the main ask; references session-model.
3. `docs/guide/multi-account.md` — deep dive on account/failover.
4. `docs/guide/subcommands.md` — command reference.
5. `docs/guide/environment-variables.md` — reference table.
6. `docs/guide/README.md` — index (written last so TOC links are final).
