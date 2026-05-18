# `codex-session` — Refactor Implementation Plan

Master plan for refactoring `codex-session` to align with the canonical
external CLI specifications. Each phase is a self-contained markdown
file sized for a single `/prex` workflow run
(plan → claude-review → implement → claude-review).

## Mission

Refactor `codex-session` (a Rust CLI wrapper around the `codex` binary)
so that every divergence from the two canonical spec trees below is
closed. The repo's existing `docs/adr/`, `README.md`, `AGENTS.md`,
`CLAUDE.md`, and in-source `//!` headers are **not** the source of
truth — they may be out of date or aspirational. The two external
specs below are.

## Single source of truth (SoT) policy

This project has **one** SoT for CLI specifications: the canonical
external doc trees listed below. In-repo docs (`README.md`, surviving
ADRs, implementation-plan files) may only contain content that is
project-specific and **not** in the canonical specs. Anything that
would duplicate the canonical specs is deleted from this repo and
replaced with a pointer.

At plan-writing time the following ADRs were removed because their
content is covered by the canonical specs or actively reversed by
this refactor (see Phase 10 §6 for the per-ADR mapping):

- `0001-top-level-passthrough-bypasses-clap.md`
- `0002-preserve-legacy-exit-codes-1-and-2.md`
- `0004-codex-resolution-via-which-crate.md`
- `0006-unset-home-falls-back-to-root.md`
- `0007-no-global-verbosity-flag.md`
- `0008-skip-directories-and-figment.md`
- `0009-foo-dot-rs-over-mod-rs.md`
- `0010-paths-stay-as-pathbuf.md`
- `0011-wrapper-env-vars-and-exit-codes.md`
- `0012-two-layer-logging-and-json-output.md`

Two ADRs survive because their content is project-specific and not
covered by the canonical specs:

- `0003-install-via-cargo-install.md` (deployment flow)
- `0005-atomic-write-via-named-tempfile.md` (merge service internal)

The root `README.md` will be rewritten in Phase 10 to point at the
canonical specs for everything they cover.

## Canonical specifications (single source of truth)

- General CLI design:
  `/home/gu/Projects/docs-n-notes/tech/programming/cli-design/`
  - `00-architecture.md` — directory roles, `AppContext`,
    parse-shape vs runtime-shape, four-edit rule.
  - `01-logging-and-output.md` — stdout/stderr discipline, color
    precedence (`NO_COLOR > FORCE_COLOR/CLICOLOR_FORCE > isatty > CLICOLOR`),
    two-layer logging.
  - `02-error-messages.md` — typed error layering, structured fields.
  - `03-config-precedence.md` — `cli > env > project > user > defaults`,
    XDG, `figment`.
  - `06-cli-wrapper-design/` — wrapper-specific rules.
    - `README.md` — overview.
    - `process-and-posix.md` — argv layout (§1), child resolution
      (§4), subcommand namespacing (§5), UX (§7), exit codes (§8),
      testability (§9).
    - `typing-and-validation.md` — typed argv builder, `Executable`
      trait, `to_args()/into_command()` boundary.
    - `checklist.md` — shippable wrapper checklist.

- Rust-specific spec:
  `/home/gu/Projects/docs-n-notes/tech/languages/rust/cli-spec/`
  - `00-directory-tree.md` — canonical `src/` layout.
  - `02-subcommand-pattern.md` — root `Cli`/`GlobalArgs`/`Commands`.
  - `03-error-handling.md` — `thiserror` + `anyhow` layering.
  - `04-logging.md` — `tracing-subscriber` + `tracing-appender`.
  - `05-config.md` — `figment` + `directories` + `deny_unknown_fields`.
  - `06-testing.md` — `assert_cmd` + `insta`.
  - `07-dependencies.md` — required crate list.
  - `08-naming-and-visibility.md` — module headers, doc coverage.
  - `09-coding-style.md` — `println!`/`eprintln!` only in `ui/`.

Every phase file cites the relevant spec sections by path and (where
possible) line numbers. Codex/claude must re-read those sections at the
start of each phase rather than trusting summaries in this repo.

## Key decisions (already made — do not relitigate)

The decisions below were settled across the conversation that produced
this plan. They are inputs to every phase, not negotiables.

1. **`self` namespace usage.** `self` is reserved exclusively for
    self-modifying operations against the wrapper binary
    (e.g. `self update`, `self uninstall`). It is **never** used for
    introspection (`version`, `help`, `completion`, `config`). This
    matches the corrected canonical doc (the prior `cargo metadata`
    example was factually wrong — `cargo` has no `cargo self`
    subcommand) and the observed pattern across `rustup`, `uv`, `gh`,
    `kubectl`, `git`, `gcloud`, `op`, `flyctl`. In this refactor `self`
    is deferred — no `self ...` subcommand ships until a real
    `self update` / `self uninstall` is implemented.

2. **Target top-level CLI surface:**

    ```
    codex-session [WRAPPER-OPTS] <verb|positional> [--] [CHILD-ARGS...]
    ```

    Wrapper-owned verbs (all top-level, no `self`):

    - `codex-session version [--format text|json]`
    - `codex-session help` (and `--help`)
    - `codex-session config status [--format text|json]`
    - `codex-session config merge`
    - `codex-session config show-local [--format text|json]`
    - `codex-session completion <shell>` (new; static completions via
      `clap_complete`)

    Anything else is forwarded verbatim to the real `codex`.
    `--` is a POSIX end-of-options sentinel (`process-and-posix.md:32-38`).

3. **Wrapper-owned `--help` and `--version`.** Currently fall through
    to the child. After the refactor, `--help` prints the wrapper's
    help and `--version` prints `codex-session 0.1.0` plus the
    resolved child path and child version line. Per
    `process-and-posix.md:248-260`.

4. **Top-level wrapper options.** Long-only, namespaced
    (`process-and-posix.md:42-45`). Short-only exceptions: `-v` (count,
    universal convention) and `-q` (universal convention).

5. **Layered config via `figment`.** Per
    `cli-design/03-config-precedence.md:7-43` and
    `rust/cli-spec/05-config.md`.
    `cli > env > project (./.codex-session/config.toml) > user
    (~/.config/codex-session/config.toml) > defaults`.

6. **XDG paths via `directories`.** No hand-rolled
    `HOME`/`XDG_*` reads (`rust/cli-spec/07-dependencies.md:24`).

7. **Typed child invocation + `Spawner` trait.** Per
    `06-cli-wrapper-design/typing-and-validation.md` and
    `process-and-posix.md:301-308`. Argv translation is a pure
    `into_command()` boundary. `--dry-run` prints resolved
    binary + argv + scrubbed env without exec'ing.

8. **Recursion guard on the exec path** (not just the `--version`
    probe). Either inode self-check on `resolve_codex`, or
    `CODEX_SESSION_REENTRY=1` marker. Per `process-and-posix.md:164-182`.

9. **Env scrubbing.** `CODEX_SESSION_*` removed from the child env
    before `exec`. Per `process-and-posix.md:343-345`.

10. **Logging.** `tracing-appender` (non-blocking, rolling) replaces
    the current `Arc<Mutex<File>>` writer. Per
    `rust/cli-spec/04-logging.md:21-25` and `07-dependencies.md:21`.
    Add `-q/--quiet/--silent` and `--log-format text|json`.

11. **Color policy.** `NO_COLOR > FORCE_COLOR/CLICOLOR_FORCE > isatty
    > CLICOLOR`. Per `cli-design/01-logging-and-output.md:25-29, 53-66, 80-97`.

12. **No `println!`/`eprintln!`/raw `stdout.write_all` outside
    `src/ui/`.** Per `rust/cli-spec/09-coding-style.md:80-89`. The
    current `src/main.rs:80-83` violation is fixed in Phase 07.

13. **Error model.** `AppError::Usage(String)` → `Usage(clap::Error)`.
    Add `Config(#[from] ConfigError)`. Expand structured logging
    fields (`err.path`, `err.line` from `figment` provenance).

## Phase index

| Phase | File | Goal | Touches |
|---|---|---|---|
| 01 | [`phase-01-root-cli.md`](phase-01-root-cli.md) | Introduce root `Cli` parser; restore wrapper-owned `--help`/`--version`; delete `maybe_parse_self_cli`/`scan_self_globals`/`DeferredEarlyExit`; handle `--`. | `src/cli.rs` → `src/cli/mod.rs`, `src/main.rs` |
| 02 | [`phase-02-de-self.md`](phase-02-de-self.md) | Lift all `self_*` subcommands to top level; introduce `config` subtree; rename modules, structs, tests, help text. | `src/cli/`, `src/commands/`, `tests/`, `src/ui/` |
| 03 | [`phase-03-config-layer.md`](phase-03-config-layer.md) | Add `src/config/` with `figment` + `directories` + `camino`. Replace `domain/paths.rs` env reads. Thread `Config` through `AppContext`. | `src/config/`, `src/context.rs`, `src/domain/paths.rs`, `Cargo.toml` |
| 04 | [`phase-04-typed-child-invocation.md`](phase-04-typed-child-invocation.md) | Add `ChildInvocation` (`src/domain/`) + `Spawner` trait (`src/adapters/`). Implement `--dry-run` printing resolved invocation. | `src/domain/child_invocation.rs`, `src/adapters/spawner.rs`, `src/main.rs`, `src/commands/pass_through.rs` |
| 05 | [`phase-05-recursion-guard-env-scrub.md`](phase-05-recursion-guard-env-scrub.md) | Move recursion guard onto the exec path; scrub `CODEX_SESSION_*` env from the child. Add tests. | `src/adapters/spawner.rs`, new `tests/cmd_recursion_guard.rs` |
| 06 | [`phase-06-logging.md`](phase-06-logging.md) | Replace `Arc<Mutex<File>>` log sink with `tracing-appender` non-blocking writer; add `-q`/`--quiet`/`--silent` and `--log-format`. | `src/logging.rs`, `src/cli/mod.rs`, `Cargo.toml` |
| 07 | [`phase-07-output-discipline.md`](phase-07-output-discipline.md) | Move all human-visible output through `Ui`. Implement full color policy precedence. | `src/ui/`, `src/main.rs` |
| 08 | [`phase-08-error-model.md`](phase-08-error-model.md) | `Usage(clap::Error)`; `ConfigError` rung; structured `err.path`/`err.line` from `figment` provenance. | `src/error.rs`, `src/config/error.rs` |
| 09 | [`phase-09-tests.md`](phase-09-tests.md) | `insta` snapshots for `--help`/`--version`; signal/`128+N` matrix; golden argv via stub child; config-precedence matrix. | `tests/`, new `tests/fixtures/echo-argv.sh` |
| 10 | [`phase-10-polish.md`](phase-10-polish.md) | `clap_complete` completion subcommand; flip `missing_docs = "warn"`; add module headers ("what it is / what it isn't"); ADR sweep. | `src/cli/completion.rs`, all `src/*.rs`, `Cargo.toml`, `docs/adr/` |

## Phase ordering and dependencies

```
01 ─┬─ 02 ──┬─ 03 ─┬─ 04 ─┬─ 05
    │      │      │      │
    │      │      │      └─ 06
    │      │      │
    │      │      └────────── 07
    │      │
    │      └────────────────── 08
    │
    └────────────────────────── 09
                                │
                                10
```

- Phase 01 unblocks every later phase (everything else assumes a real
  root `Cli`).
- Phase 02 unblocks 03 because the `config` subtree is where the
  loaded `Config` value gets exercised first.
- Phase 03 unblocks 04 because `Spawner` resolves the child binary
  from `Config`.
- Phases 04–07 can run in roughly parallel order after 03, but the
  written plan is sequential to keep prex runs simple.
- Phase 08 depends on 03 (`ConfigError` requires `Config`).
- Phase 09 (tests) and Phase 10 (polish) come last and lock the
  contract.

## How to execute each phase

1. From the repo root, run:

    ```bash
    /prex -a $(cat docs/implementation-plan/phase-NN-*.md)
    ```

    (`-a` for auto-approve after Claude reviews the Codex plan. Drop
    the flag if you want to gate stage 2 manually. Use `-ar` for the
    full review loop.)

2. The phase file becomes Codex's task description. Codex plans;
    Claude reviews the plan against the canonical specs; Codex
    implements; Claude reviews the implementation.

3. After the phase completes, run `cargo check`, `cargo test`,
    `cargo clippy --all-targets -- -D warnings`. The acceptance
    criteria in each phase file must all be green.

4. Commit with a Conventional Commit message
    (`refactor(cli): phase 01 — root Cli + wrapper-owned --help/--version`),
    then move to the next phase.

## Rules for each phase file (so prex runs cleanly)

Each phase file follows this structure:

1. **Title + one-sentence summary.**
2. **Prerequisites** — which earlier phases must be complete.
3. **Goal** — what the codebase looks like after the phase.
4. **Spec rationale** — citations into the canonical docs.
5. **Current state** — concrete `file:line` references from the repo
    as of the plan-writing date (re-verify; line numbers drift).
6. **Target state** — file layout and signatures after the phase.
7. **Tasks** — ordered, granular steps Codex should execute.
8. **Acceptance criteria** — checklist for claude-review.
9. **Tests** — what to add/update.
10. **Out of scope** — bright lines so Codex does not over-build.
11. **References** — relevant canonical-spec paths.

Codex must re-read the cited spec sections at planning time. Line
numbers in this plan are snapshots and will drift; the spec content
controls.

## Out of scope for the entire refactor

- `codex-session self update` / `self uninstall` (the `self`
  namespace is reserved but no implementation ships).
- Workspace migration (single-crate stays per
  `cli-design/00-architecture.md` until ≥8k LOC or a second consumer).
- `tokio` runtime (this wrapper is fully synchronous; no async needed).
- `color-eyre` pretty errors (defer until dev ergonomics demand it).
- Plugin system (`codex-foo` PATH-dispatch).

## Glossary

| Term | Meaning |
|---|---|
| **wrapper** | The `codex-session` binary. |
| **child** | The real `codex` binary, resolved via override → PATH → bundled. |
| **passthrough** | Forwarding a non-wrapper-owned invocation to the child via `exec()`. |
| **parse-shape** | The clap struct (`String`, `Option<String>`, `bool`). |
| **runtime-shape** | The domain types the handler uses after parsing (newtypes, enums, validated paths). |
| **AppContext** | Single value built once in `main`, threaded by reference. Holds `Config`, `Paths`, resolved child, `Ui`. |
| **adapter** | I/O at the edges (one trait + default impl per external system). |
| **spawner** | The exec-layer trait (`Spawner`). Hides `fork/exec/wait` so tests can stub. |
| **child invocation** | The typed `{ binary, args, env }` value passed to `Spawner`. |

## Notes for Codex (read at planning time)

- The repo is `/workspaces/codex-session/`.
- Current branch: `1-rust-migration-migrate-from-bash-to-rust`.
- Working tree is dirty at plan-writing time; ignore the pre-existing
  diffs and base each phase on the current `HEAD` at the time the
  phase runs.
- Do not commit anything yourself — the Claude Code orchestrator
  handles all git operations.
- Run `cargo check && cargo clippy --all-targets -- -D warnings &&
  cargo test` before declaring a phase complete. Update snapshots
  with `cargo insta review` when behavior intentionally changes.
- The `justfile` (replaces the old `Makefile`) is the local task
  runner — prefer `just check`, `just test` etc. when available.
- MSRV is `1.85`; edition is `2024`.
- Lints: `unsafe_code = "forbid"`. Production code must be free of
  `unwrap()` / `expect()` / `panic!()` (currently `warn`, will be
  upgraded in Phase 10).
