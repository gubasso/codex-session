# 05 — CLI Design Conformance

This doc captures the wrapper-CLI design decisions, checked against:

- `/home/gu/Projects/docs-n-notes/tech/programming/cli-design/06-cli-wrapper-design/{README,process-and-posix,typing-and-validation,checklist}.md`
- `/home/gu/Projects/docs-n-notes/tech/languages/rust/cli-spec/02-subcommand-pattern.md`
- `/home/gu/Projects/docs-n-notes/tech/programming/cli-design/03-config-precedence.md`
- `/home/gu/Projects/docs-n-notes/tech/programming/cli-design/02-error-messages.md`

## Subcommand tree (post-refactor)

```
codex-session [GLOBAL-FLAGS] <verb> [...]
│
├── (intrinsic wrapper-owned — already present pre-refactor)
│   ├── version
│   ├── completion
│   ├── config
│   │   └── status
│   ├── profile
│   │   ├── list
│   │   ├── show
│   │   └── compose
│   └── doctor                      ← R1 extends output: account, group-id, codex_home
│
├── (NEW — Round 2 onward)
│   └── account
│       ├── add <name> [--from-native]
│       ├── list [--json]
│       ├── current [--json]
│       ├── use <name>              (pin; writes state/last-account)
│       ├── remove <name>           (archives to accounts/.trash/)
│       ├── quota [--live] [--json] [--account <name>|--all]   ← R3
│       └── cooldown
│           ├── show [--json]
│           └── clear [--account <name>|--all]
│
└── (anything else → passes through to codex unchanged)
    exec, apply, resume, fork, review, login, logout, mcp, plugin, ...
```

## Why `account` is top-level (not `self account`)

Applying `process-and-posix.md` §5.1:

> `self` is justified only when (1) the verb's object is the running binary itself — it updates, uninstalls, or otherwise mutates the wrapper — AND (2) the same verb name plausibly exists on the wrapped child.

Stripping `self` from `self account list` → `account list`. Still unambiguous. No collision with codex's verb set: `exec / review / login / logout / mcp / plugin / mcp-server / app-server / remote-control / completion / update / doctor / sandbox / debug / apply / resume / fork / cloud / exec-server / features / help` (verified from `codex --help`). Therefore `self` adds noise. ADR [D8](04-decisions.md) records this; the wrapper's existing verbs (`version`, `completion`, `config`, `profile`, `doctor`) already follow the same top-level convention.

### Precedent survey (§5.3 of `process-and-posix.md`)

| Tool | management verbs at top level? | uses `self`? |
|---|---|---|
| `gh` | ✅ `gh auth login/...`, `gh extension install/...`, `gh config` | ✗ |
| `kubectl` | ✅ `kubectl config get-contexts/...` | ✗ |
| `gcloud` | ✅ `gcloud auth list/...`, `gcloud config set/...` | ✗ |
| `git` | ✅ `git config`, `git stash list/...` | ✗ |
| `cargo` | ✅ no `self` namespace anywhere | ✗ |
| `op` (1Password) | ✅ `op account add/list/use/...` ← closest precedent for our shape | ✗ |
| `flyctl` | ✅ | ✗ |
| `rustup` | — | ✅ only for `self update` / `self uninstall` |
| `uv` | — | ✅ only for `self update` |

## Global flags (wrapper-owned)

All wrapper-owned global flags are **long-form only and prefixed-namespaced**, per `process-and-posix.md` §1 ("avoid future collisions with upstream flags; leave short flags to the child"). They are parsed *before* the verb token.

| Flag | Type | Default | Env mirror | Where wired |
|---|---|---|---|---|
| `--group <id>` | `String` | (resolution chain) | `CODEX_SESSION_GROUP` | `cli::GlobalArgs` |
| `--account <name>` | `String` (or `"auto"` sentinel) | (resolution chain) | `CODEX_SESSION_ACCOUNT` | `cli::GlobalArgs` |
| `--max-retries <N>` | `u32` | `0` (off) | — | `cli::GlobalArgs` |
| `--` | sentinel | — | — | clap built-in |

No new short flags. Codex's short flags (`-c`, `-m`, `-i`, `-p`, `-h`, `-V`) are reserved for codex and pass through.

## Argv handling — the denylist rule

Per `process-and-posix.md` §6 ("default to verbatim pass-through; translate only when you must") and §10 ("greedy flag consumption" anti-pattern), the wrapper parses the **minimum subset** of argv:

1. Global flags listed above.
2. The verb token, if it matches the wrapper's denylist of owned verbs: `{version, completion, config, profile, doctor, account, --help, -h, --version, -V}` → wrapper handles.
3. Anything else → forwarded verbatim to codex via the existing `External(argv)` mechanism.

The wrapper does **NOT** parse codex's own grammar (`-c key=value`, `--enable feature`, `-m model`, etc.) — those flags reach codex untouched, even if they syntactically clash with wrapper flag names (the wrapper's claim ends at the verb boundary).

## Env-var scrubbing

Per `process-and-posix.md` §10 ("inheriting the wrapper's environment unfiltered into the child" anti-pattern), the wrapper-internal `CODEX_SESSION_*` namespace must not leak into the child's environment.

R1 audits `src/domain/child_invocation.rs::ChildEnv::scrubbed_default()` and confirms (or adds) explicit scrubbing of all `CODEX_SESSION_*` variables from the child's environment. The child receives:

- `CODEX_HOME` (set by us to the resolved group dir).
- The user's normal environment minus our namespace.
- Any `[env]` keys the active profile contributes (existing behavior).

R1 adds a regression test for this scrubbing.

## Exit codes (`process-and-posix.md` §8 + project `src/error.rs`)

The wrapper already uses BSD sysexits via `AppError::exit_code()`. New variants this refactor introduces:

| Variant | Code | Meaning |
|---|---|---|
| `AppError::Account(AccountError::NotFound)` | 78 (`EX_CONFIG`) | Named account not registered |
| `AppError::Account(AccountError::InvalidName)` | 64 (`EX_USAGE`) | Account name fails `[a-z0-9][a-z0-9_-]{0,31}` validation |
| `AppError::Account(AccountError::NoEligible)` | 75 (`EX_TEMPFAIL`) | All accounts cooled-down or below threshold |
| `AppError::Account(AccountError::QuotaFetch)` | 69 (`EX_UNAVAILABLE`) | `wham/usage` call failed |
| `AppError::Account(AccountError::QuotaParse)` | 65 (`EX_DATAERR`) | `wham/usage` response unparseable |

Child exit codes pass through unchanged. Signal death is `128 + N` per §3 / §8.

## Help rendering (cli-spec §02 "Help rendering with clap")

**Tier 1 only.** Each subcommand has `about = "..."` on its `clap::Args` struct. Long-form rationale lives in `src/ui/help_extras.txt`, wired via `#[command(after_long_help = include_str!("../ui/help_extras.txt"))]` on the root `Cli`.

R2 creates / extends `src/ui/help_extras.txt` to document:

- Passthrough semantics (`unknown verbs go to codex`).
- The `--` end-of-options sentinel.
- Env vars: `CODEX_SESSION_GROUP`, `CODEX_SESSION_ACCOUNT`, `CODEX_SESSION_CHILD_BIN`.
- The `account` verb tree with one-line per sub-verb.
- The `--account auto` selector (R3+).
- "See also: `codex --help` for codex's own verb/flag reference."

No Tier-2 (`clap-help`) or Tier-3 (`disable_help_flag`) escalation. Surface is small enough.

## Four-edit rule (cli-spec §02)

Every new subcommand obeys: `src/cli/<verb>.rs` (parse-shape) + `src/cli/mod.rs` (register `Commands::<Verb>` variant) + `src/commands/<verb>.rs` (handler) + `src/commands/dispatch.rs` (arm).

For `account` (which has its own sub-verbs), the handler lives under `src/commands/account/` — same pattern as the existing `src/commands/profile_*.rs` triad and `src/commands/config_status.rs`. Sub-verbs each get their own file (`add.rs`, `list.rs`, `current.rs`, `use_.rs`, `remove.rs`, `quota.rs`, `cooldown.rs`).

## Typed-model boundary (`typing-and-validation.md`)

`AccountId` is a newtype around `String` with `FromStr` validation: regex `[a-z0-9][a-z0-9_-]{0,31}`. Parsing happens once at the CLI boundary (`AccountArgs::name: AccountId`); downstream code never sees raw strings.

Similar newtype for `GroupId` (same regex; validation lives in `services/session/group_id.rs`).

## Config precedence (`03-config-precedence.md`)

Standard ladder, top wins: `CLI > env > project file > user file > defaults`. The `[account]` section follows the same pattern. Specifically for account resolution:

1. `--account` flag (CLI).
2. `CODEX_SESSION_ACCOUNT` env.
3. `[account] pinned = ...` in project file (`.codex-session/config.toml`).
4. `[account] pinned = ...` in user file (`$XDG_CONFIG_HOME/codex-session/config.toml`).
5. `[account] default = ...` in user file.
6. Literal `"default"`.

The `Config` struct already tracks `ConfigSources`; the new `[account]` section just slots into that machinery.

## Wrapper checklist conformance (`process-and-posix.md` "one-screen start-from-scratch checklist")

| Item | Status |
|---|---|
| Wrapper flags long & namespaced | ✅ `--group`, `--account`, `--max-retries` |
| `--` honored as end-of-options | ✅ (clap built-in, existing behavior) |
| Documented shape: `wrapper [WRAPPER] verb [-- CHILD-ARGS...]` | ✅ in `ui/help_extras.txt` (R2) |
| Unknown flag *before* `--` → error; *after* → forwarded | ✅ (existing) |
| Precedence: flag > env > project > user > defaults | ✅ |
| All wrapper env vars share one prefix (`CODEX_SESSION_*`) | ✅ (existing) |
| `CODEX_SESSION_CHILD_BIN` overrides binary resolution | ✅ (existing) |
| Default to `execvp`; spawn when post-processing needed | We spawn (need pre/post-flight); documented |
| Spawn: forward INT/TERM/QUIT/HUP/TSTP/CONT/USR1/USR2/WINCH | ✅ (existing `services/auth/signal.rs` — kept until R4 cleanup; signal forwarding may need to move to `commands/pass_through.rs` if `signal.rs` is deleted) |
| Exit = child's; signal-death = 128+N | ✅ (existing) |
| Resolved child path logged under verbose | ✅ (existing structured logging) |
| Missing child → 127; not executable → 126 | ✅ (existing) |
| Plugins / sub-verbs: ONE model picked | ✅ Reserved-verb namespace **not** used (rejected — see D8). Top-level wrapper-owned verbs only. |
| `self` reserved *only* for binary-mutating verbs | ✅ (we have none; we use `self` for nothing) |
| `version` / `help` / `completion` / `config` / `doctor` at top level | ✅ (existing); `account` joins them at top level |
| `--help` shows wrapper opts + how to reach child help | ✅ (R2 updates `help_extras.txt`) |
| `--version` prints wrapper + child version + resolved child path | ✅ (existing `version` command) |
| Wrapper diagnostics → stderr; child stdout/stderr untouched | ✅ (existing); R4 detector tees, doesn't filter |
| Completions for wrapper flags | ✅ (existing `completion` verb); R2 regenerates after account verbs added |
| Exit codes documented | ✅ in `error.rs` + this doc |
| Test seams: Spawner abstraction, fake codex, snapshot help/version | ✅ (existing `tests/support::TestEnv`) |
