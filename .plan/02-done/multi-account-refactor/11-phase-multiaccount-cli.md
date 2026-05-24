# Round 2 — Multi-account top-level CLI + account-aware path resolution

This file is the **prex input** for Round 2. Pass its contents verbatim to `/prex -a`.

---

## Prerequisite

Round 1 (`10-phase-foundation.md`) has landed and shipped. The wrapper currently exports `CODEX_HOME = <state-root>/accounts/default/groups/<group-id>/` for every invocation.

## Goal

Add the **top-level** `account` subcommand with sub-verbs `add / list / current / use / remove`. Add `[account]` config section. Wire account-aware path resolution. No quota or selector logic yet — account selection is explicit (env / flag / pinned / default) until Round 3.

**`--account auto` is accepted in this round** but no-ops with a warning ("auto selector requires Round 3, falling back to pinned account"). Wiring it fully lands in R3.

## Background (read before planning)

- `.plan/multi-account-refactor/00-overview.md` — north star, glossary.
- `.plan/multi-account-refactor/01-architecture.md` — account resolution chain.
- `.plan/multi-account-refactor/04-decisions.md` — ADR **D8** (top-level `account` verb, no `self` prefix).
- `.plan/multi-account-refactor/05-cli-design.md` — full subcommand tree, global flags, conformance checklist.
- `/home/gu/Projects/docs-n-notes/tech/languages/rust/cli-spec/02-subcommand-pattern.md` — the four-edit rule + help rendering tiers.
- `/home/gu/Projects/docs-n-notes/tech/programming/cli-design/06-cli-wrapper-design/process-and-posix.md` §5 (sub­command namespacing).
- Existing `src/cli/profile.rs` + `src/commands/profile_{list,show,compose}.rs` — the **template** for this round's `account` shape. Match the pattern.
- `src/cli/argv.rs::legacy_self_invocation` — do **NOT** loosen the legacy `self` reject. R2 adds top-level verbs, not `self <verb>`.

## Numbered implementation steps

1. **Add `cli::Commands::Account(AccountArgs)` variant.**
    - New file `src/cli/account.rs` (parse-shape only; matches existing `src/cli/profile.rs`).
    - `AccountArgs` has `#[command(subcommand)] sub: AccountSubcommand`.
    - `AccountSubcommand` enum variants: `Add(AddArgs)`, `List(ListArgs)`, `Current(CurrentArgs)`, `Use(UseArgs)`, `Remove(RemoveArgs)`.
    - Each sub-args struct has its own `#[derive(clap::Args)]` block with `about = "..."` per the cli-spec Tier 1 help convention.
    - Register the variant in `src/cli/mod.rs::Commands` with `about = "Manage codex-session accounts (multi-credential pool)"`.

2. **Create `src/commands/account/` module** with one file per sub-verb (mirroring `src/commands/profile_*.rs` granularity, but nested under `account/` since they share a parent verb):
    - `mod.rs` — re-exports + `pub(crate) fn dispatch(ctx, args)` that matches on `AccountSubcommand` and calls the right handler.
    - `add.rs` — `pub(crate) fn run(ctx, args: &AddArgs) -> Result<(), AppError>`.
    - `list.rs` — same shape.
    - `current.rs`, `use_.rs` (`use` is a reserved keyword), `remove.rs` — same.
    - Each handler is short (~30–60 LOC): projects `AddArgs` etc. into a service-layer call, renders via `ctx.ui`, returns typed error. **No I/O in the handlers themselves** — delegate to `services::account::registry`.

3. **Wire dispatch in `src/commands/dispatch.rs`.**
    - Add an arm: `Some(cli::Commands::Account(args)) => commands::account::dispatch(ctx, args).map(|()| 0),`.

4. **Add `--account <name>` global flag.**
    - In `src/cli/mod.rs::GlobalArgs`, add `pub(crate) account: Option<AccountSelector>` where `AccountSelector` is an enum: `Named(AccountId) | Auto`. Custom `FromStr`: `"auto"` → `AccountSelector::Auto`; anything else → parses through `AccountId::from_str`.
    - Env mirror: `CODEX_SESSION_ACCOUNT`. Apply the same mirroring pattern the existing `--profile` flag uses.

5. **Add `[account]` section to `Config` (`src/config/mod.rs`).**
    - `pub(crate) struct AccountConfig { pub default: Option<AccountId>, pub pinned: Option<AccountId>, pub registry_dir: Option<Utf8PathBuf> }`.
    - Default `registry_dir` derived from `Paths::state_dir` as `<state>/accounts/`.
    - Env layer reads `CODEX_SESSION_ACCOUNT_DEFAULT`, `CODEX_SESSION_ACCOUNT_PINNED`, `CODEX_SESSION_ACCOUNT_REGISTRY_DIR` (the double-underscore-nesting pattern is overkill for three keys).
    - Add new `ConfigError::AccountConfig(...)` variants for parse failures.

6. **New service `src/services/account/registry.rs`.**
    - Filesystem-backed registry rooted at `<state>/accounts/`.
    - `pub(crate) struct Registry { root: Utf8PathBuf }`.
    - `pub(crate) fn list(&self) -> Result<Vec<AccountEntry>, RegistryError>` — scans subdirs, excludes `.trash/`, returns `AccountEntry { id: AccountId, dir: Utf8PathBuf, has_auth: bool, last_used_at: Option<SystemTime> }`. `last_used_at` is `mtime` on `<root>/<name>/groups/`.
    - `pub(crate) fn add(&self, name: &AccountId, from_native: bool, native_home: &Utf8Path) -> Result<(), RegistryError>` — creates `<root>/<name>/`. If `from_native`, copy `~/.codex/auth.json` into a `groups/<group-id>/` skeleton or to `<root>/<name>/auth.json` (decide: the per-account `auth.json` is conceptually account-scoped, not group-scoped, so storing it ABOVE the `groups/` level — at `<root>/<name>/auth.json` — and symlinking into each group-dir on first use is one option; OR storing it inside the first-used group-dir is another. **Recommend: store at `<root>/<name>/auth.json` and symlink/copy into the group-dir on first use, so all groups for an account share one auth file**).
    - `pub(crate) fn remove(&self, name: &AccountId) -> Result<(), RegistryError>` — moves `<root>/<name>/` → `<root>/.trash/<name>-<ts>/`. Non-destructive.
    - `pub(crate) fn current(&self) -> Result<Option<AccountId>, RegistryError>` — reads `<state>/state/last-account`.
    - `pub(crate) fn set_current(&self, name: &AccountId) -> Result<(), RegistryError>` — atomic write to `<state>/state/last-account`.

7. **New service `src/services/account/resolver.rs`.**
    - `pub(crate) fn resolve(ctx: &AppContext) -> Result<AccountId, AccountError>`.
    - Chain (top wins):
      1. `ctx.global.account` (CLI flag).
      2. `std::env::var("CODEX_SESSION_ACCOUNT")` (note: already mirrored to flag via clap's env support, but redundant check is fine).
      3. `ctx.config.account.pinned`.
      4. `ctx.config.account.default`.
      5. Literal `AccountId::default()` (returns `"default"`).
    - If any arm produces `AccountSelector::Auto`: in Round 2, warn via `ctx.ui` ("auto selector requires Round 3, falling back to pinned/default"); proceed with the next arm. Round 3 wires `auto` to `selector::pick()`.

8. **Wire account resolution into `src/services/session/dir.rs::session_dir`.**
    - Caller in `pass_through.rs` now reads `account = resolver::resolve(ctx)?` and passes it to `session_dir(root, &account, &group_id)`.
    - Remove the hardcoded `"default"` from R1.

9. **Update `src/commands/doctor.rs`.**
    - Add a new "accounts" section listing registered accounts and which is currently active (with source: flag / env / pinned / default / fallback).
    - JSON output gains `accounts: [{name, has_auth, last_used_at}, ...]` and `active_account: {name, source}`.

10. **Create / extend `src/ui/help_extras.txt`** and wire `#[command(after_long_help = include_str!("../ui/help_extras.txt"))]` on `Cli` in `src/cli/mod.rs`.
    - Content sections:
      - `WRAPPER OVERVIEW` — one paragraph: "codex-session wraps `codex` with persistent CODEX_HOME, multi-account support, and quota-aware selection."
      - `WRAPPER VERBS` — bulleted: `version`, `completion`, `config`, `profile`, `doctor`, `account`. One-liner each.
      - `PASSTHROUGH` — "Any verb not listed above is forwarded verbatim to codex. Use `--` to disambiguate."
      - `ACCOUNT MANAGEMENT` — `codex-session account add work --from-native`, `account list`, `account use <name>`, `--account <name>` flag.
      - `ENV VARS` — `CODEX_SESSION_GROUP`, `CODEX_SESSION_ACCOUNT`, `CODEX_SESSION_CHILD_BIN`.
      - `SEE ALSO` — "Run `codex --help` for codex's own verbs and flags."

11. **Tests.** New `tests/account_*.rs` files (~5–6 tests):
    - `account_add_creates_dir` — `codex-session account add work` creates `<state>/accounts/work/`; second add errors.
    - `account_add_from_native_copies_auth` — `--from-native` copies `~/.codex/auth.json`.
    - `account_list_shows_registered` — lists accounts; `.trash/` excluded.
    - `account_use_pins_lru` — writes `state/last-account`; subsequent `account current` reflects it.
    - `account_remove_archives` — moves to `.trash/`; original gone.
    - `account_resolver_chain` — table-driven: flag > env > pinned > default > "default".
    - `passthrough_routes_to_account_path` — `--account work exec "hi"` ends up writing rollouts under `accounts/work/groups/<gid>/`.
    - `account_id_validation` — invalid names error with `EX_USAGE` (64).
    - `auto_warns_in_r2` — `--account auto` warns and proceeds with the next arm.
    - Legacy: `tests/legacy_self_reject.rs` (or existing equivalent) — `codex-session self account list` still fails with `EX_USAGE` (legacy reject preserved).
    - Snapshot updates: `tests/cmd_root_help.rs` reflects the new `account` verb in help output.

## Files touched (representative)

- `src/cli/account.rs` (NEW), `src/cli/mod.rs` (Commands variant + GlobalArgs flag)
- `src/commands/account/{mod,add,list,current,use_,remove}.rs` (NEW module)
- `src/commands/dispatch.rs` (new arm)
- `src/config/mod.rs`, `src/config/error.rs` (new section + variants)
- `src/services/account/{mod,registry,resolver}.rs` (NEW)
- `src/services/session/dir.rs` (account argument no longer hardcoded)
- `src/error.rs` (new `AppError::Account(AccountError)` variant + exit-code mapping per `05-cli-design.md`)
- `src/ui/help_extras.txt` (NEW or extended)
- `tests/account_*.rs` (~5–6 new files)
- `tests/cmd_root_help.rs` (snapshot update)

**Net LOC estimate:** ~550–700. **New tests:** ~10–12.

## Done criteria

```sh
just precommit-all     # must exit 0
```

Plus manual smoke:

```sh
codex-session account add work --from-native
codex-session account add personal --from-native
codex-session account list
codex-session --account work exec "echo hi"            # routes to accounts/work/groups/<gid>/
codex-session account use personal
codex-session exec "echo hi"                            # routes via pin
codex-session account current --json | jq .            # {"name": "personal", "source": "lru"}
codex-session account remove work
ls ~/.local/state/codex-session/accounts/                # work/ gone, .trash/work-<ts>/ present
codex-session --account auto exec "hi" 2>&1 | grep "auto selector requires"   # warns
codex-session self account list                          # MUST FAIL (legacy reject preserved)
```

## Out of scope for Round 2

- Quota reader / `wham/usage` HTTP client (Round 3).
- `--account auto` selector logic (Round 3).
- `account quota` sub-verb (Round 3).
- `account cooldown` sub-verb (Round 4).
- Reactive 429 failover (Round 4).
- Deleting `auth/watcher.rs`, `auth/signal.rs` (Round 4).

## Constraints

- Use `just` recipes, not raw `cargo`.
- Four-edit rule (cli-spec §02): one file per subcommand on both sides.
- Tier 1 help rendering only — no `clap-help`, no `disable_help_flag`.
- `AccountId` newtype with `FromStr` validation against `[a-z0-9][a-z0-9_-]{0,31}`. Parse once at the CLI boundary; downstream sees only `AccountId`.
- `pub(crate)` everywhere.
- New structured-log `op=` keys: `account.add`, `account.use`, `account.remove`, `account.resolve`.
- Errors via `AppError::Account(AccountError)` with exit codes per `05-cli-design.md`.
