# Round 01: CLI Design System Specification

> Plan: cli-design-system-colorful-output | Round: 01 of 03 | Complexity: L | Generated: 2026-05-26
> Repo: /workspaces/codex-session

## Context

codex-session is a CLI wrapper around OpenAI's `codex` binary. It adds multi-account management,
config-recipe-layered session composition, quota-aware account selection, and health monitoring. The
wrapper produces its own output for ~15 wrapper-owned commands (account list/current/health/quota/
cooldown, doctor, config status, config_recipe list/show/compose, version) plus error rendering and
runtime narration (authentication warnings, retry messages, interactive prompts).

Currently, output quality varies wildly across commands. `account quota` has colored progress bars,
human-readable timestamps, rank/score display, and conditional ANSI — it's the gold standard.
Meanwhile `account list` dumps raw key=value pairs, `doctor` has a plain-text table, and error
messages use ad-hoc bold labels. There is no shared reference defining what colors mean, how tables
should be laid out, or how the two output channels (user-facing vs structured logs) should differ.

This round creates the authoritative CLI Design System specification document. The spec will be the
single source of truth for all visual output decisions in subsequent rounds. No code is changed in
this round.

## Previous Rounds

This is the first round — no prior rounds.

## Scope of This Round

**IN scope:**
- Write `docs/design/cli-style-guide.md` — the main design system specification
- Optionally write `docs/design/color-reference.md` if the color catalog warrants a separate file
- Add a pointer to the spec in `CLAUDE.md` so agents and contributors know to consult it
- Add a note in `CLAUDE.md` about the pre-v1.0 breaking-changes policy

**OUT of scope:**
- Any code changes to `src/`
- Any changes to `Cargo.toml` or dependencies
- Test changes
- Actually applying the design system to commands (that's rounds 02 and 03)

## Current State

### Key Files

- `/workspaces/codex-session/src/ui/mod.rs` — The main UI rendering module (917 lines). Contains
  all `write_*` methods on the `Ui` struct, the `quota_styles` module with color constants, helper
  functions `style_open()`/`style_close()`, time formatting (`human_age`, `human_duration_until`,
  `human_duration_secs`), and the `quota_bar()` progress bar renderer.

- `/workspaces/codex-session/src/ui/color.rs` — Color policy module (141 lines). Single source of
  truth for whether ANSI color is enabled. Respects `NO_COLOR`, `FORCE_COLOR`, `CLICOLOR_FORCE`,
  `CLICOLOR` environment variables and TTY detection. Key functions: `should_color(Stream)`,
  `stderr_color()`, `stdout_color_choice()`.

- `/workspaces/codex-session/src/error.rs` — Error rendering (933 lines). The `render()` function
  produces structured user-facing error output with `codex-session:`, `where:`, `why:`, `hint:`,
  and `caused by:` labels. Already uses bold styling for labels via `style_label()`.

- `/workspaces/codex-session/src/logging.rs` — Logging bootstrap (156 lines). Configures
  `tracing-subscriber` with a JSON file sink (always) and an optional stderr mirror (pretty or
  JSON). The file sink is always JSON with no ANSI. The stderr mirror respects `stderr_color()`.

- `/workspaces/codex-session/src/services/account/gate.rs` — Account gate/resolver with runtime
  narration. Uses `write_warning()` for auth warnings and `write_prompt()` with `[codex-session]`
  prefix for narration messages. The `narrate()` function at line 858 is the pattern.

- `/workspaces/codex-session/CLAUDE.md` — Agent guide (referenced by all coding agents). Currently
  documents quality gates (`just` recipes), upstream codex reference, but nothing about UI/output
  conventions.

### Existing Patterns

The `quota_styles` module defines the current color palette:

```rust
pub(super) const BOLD: Style = Style::new().effects(Effects::BOLD);
pub(super) const DIM: Style = Style::new().effects(Effects::DIMMED);
pub(super) const BOLD_CYAN: Style = Style::new()
    .fg_color(Some(anstyle::Color::Ansi(AnsiColor::Cyan)))
    .effects(Effects::BOLD);
pub(super) const RED: Style = Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::Red)));
pub(super) const BOLD_GREEN: Style = Style::new()
    .fg_color(Some(anstyle::Color::Ansi(AnsiColor::Green)))
    .effects(Effects::BOLD);
pub(super) const BOLD_YELLOW: Style = Style::new()
    .fg_color(Some(anstyle::Color::Ansi(AnsiColor::Yellow)))
    .effects(Effects::BOLD);
pub(super) const BOLD_RED: Style = Style::new()
    .fg_color(Some(anstyle::Color::Ansi(AnsiColor::Red)))
    .effects(Effects::BOLD);
```

Color is always gated through `style_open(style, use_color)` / `style_close(style, use_color)`
where `use_color` comes from `color::should_color(Stream)`.

Output format is controlled by `OutputFormat` enum (`Text` | `Json`), passed as `--format` global
flag. The wrapper avoids `--json` to prevent collision with upstream `codex --json` (JSONL events).

Structured logging uses `tracing::info!()`, `tracing::warn!()`, `tracing::error!()` with
`op = "..."` and `status = "..."` structured fields. File sink is always JSON; stderr mirror is
optional pretty or JSON. No ANSI in log output.

User-facing stderr messages use two methods:
- `write_warning(body)` — prints to stderr with trailing newline
- `write_prompt(body)` — prints to stderr without trailing newline (for interactive prompts)

The `[codex-session]` prefix is used in `gate.rs::narrate()` for runtime status messages.

## Implementation Steps

### Step 1: Create `docs/design/` directory structure

Create the directory:
```text
docs/design/
```

### Step 2: Write `docs/design/cli-style-guide.md`

This is the main deliverable. The document must define the complete CLI design system for
codex-session. It should be organized as a reference that implementors (human or AI) can consult
when writing any output code.

**Required sections:**

1. **Principles** — 3-5 guiding principles for CLI output (e.g., "scannable at a glance",
  "degrade gracefully without color", "two-channel separation: user-facing vs logs").

2. **Output Channels** — Formal definition of the two channels:
  - **User-facing** (stdout for command results, stderr for warnings/prompts/errors): full styling,
    human-readable times, colored status indicators, Unicode symbols.
  - **Structured logs** (tracing → file + optional stderr mirror): JSON, no ANSI, structured
    fields (`op`, `status`, `err.kind`, etc.), machine-parseable. Never rendered to the user
    unless `--log-stderr` is enabled.
  - Rules for what goes where: command results → stdout, warnings → stderr, errors → stderr,
    progress narration → stderr, debug/trace → logs only.

3. **Color Palette** — The canonical color assignments. Use ONLY the 8 basic ANSI colors (via
  `anstyle::AnsiColor`) for maximum terminal compatibility. Document each style constant, its
  semantic meaning, and when to use it:

  | Constant     | ANSI           | Semantic meaning                                |
  |--------------|----------------|-------------------------------------------------|
  | BOLD         | bold           | Entity names, emphasis                          |
  | DIM          | dimmed         | Headers, metadata, secondary info, paths        |
  | BOLD_CYAN    | bold+cyan      | Active/current item, selection indicator         |
  | GREEN        | green          | Checkmarks (✓), positive indicators             |
  | BOLD_GREEN   | bold+green     | OK status, eligible, positive values, success   |
  | BOLD_YELLOW  | bold+yellow    | Warning, unknown, pending, countdown values     |
  | RED          | red            | Error markers (✗), negative indicators          |
  | BOLD_RED     | bold+red       | Fail status, cooldown active, critical errors   |

  Include the rule: GREEN (non-bold) is for checkmarks/symbols only; BOLD_GREEN is for text
  labels and values.

4. **Color Gating** — Every color usage MUST go through `color::should_color(Stream)` →
  `style_open()`/`style_close()`. Document the `NO_COLOR` / `FORCE_COLOR` / `CLICOLOR` hierarchy.
  Emphasize: when `use_color` is false, output must be perfectly readable (no invisible text, no
  broken alignment from ANSI escape sequences).

5. **Typography & Symbols** — Standard Unicode symbols and their usage:
  - `▸` (U+25B8) — Current/active item marker (used with BOLD_CYAN)
  - `✓` (U+2713) — Positive/authenticated/OK (used with GREEN)
  - `✗` (U+2717) — Negative/no-auth/fail (used with RED)
  - Spacing: `▸` is always followed by a space; `✓`/`✗` are used inline in table cells

6. **Table Layout** — Rules for tabular output:
  - Header row: ALL CAPS, DIM styled
  - Column widths: dynamic from data, with minimum widths for each column type
  - Alignment: left-align text, right-align numbers
  - Active/current row: `▸` prefix + BOLD_CYAN
  - No trailing whitespace
  - No box-drawing characters (plain space-aligned columns)

7. **Status Indicators** — Semantic color mapping for common status values:

  | Value pattern          | Style       | Example context                 |
  |------------------------|-------------|---------------------------------|
  | ok / valid / live      | BOLD_GREEN  | token status, health status     |
  | warn / unknown / cache | BOLD_YELLOW | token unknown, cache_only       |
  | fail / invalid / error | BOLD_RED    | token invalid, cache_missing    |
  | active / current       | BOLD_CYAN   | active account, current marker  |
  | eligible               | BOLD_GREEN  | cooldown eligible               |
  | cooled-down            | BOLD_RED    | cooldown active                 |
  | true (boolean)         | depends     | context-dependent (see below)   |
  | false (boolean)        | depends     | context-dependent (see below)   |

  Boolean styling depends on context: `active: true` → BOLD_CYAN, `cooldown: true` → BOLD_RED,
  `has_auth: true` → GREEN ✓.

8. **Time Formatting** — All timestamps shown to users use `human_age()` (e.g., "2m 30s ago") or
  `human_duration_until()` (e.g., "1h 23m"). Raw unix timestamps are never shown in text mode
  (they belong in `--format json` and logs). The `human_duration_secs()` format:
  - `>= 1d`: `{d}d {h}h`
  - `>= 1h`: `{h}h {m}m`
  - `>= 1m`: `{m}m {s}s`
  - `< 1m`: `{s} s`
  - `0` or missing: `"never"` in DIM style

9. **Progress Bars** — The `quota_bar()` pattern: filled blocks `█` and empty blocks `░`, colored
  by percentage threshold (>50% green, >20% yellow, ≤20% red). Width: 20 columns. Used only for
  quota percentage displays.

10. **Error Rendering** — The standard error format (rendered by `error.rs::render()`):
    ```text
    codex-session: {what}
      where: {path} (line {N})
      why:   {why_line}
      hint:  {hint}
      caused by: {chain}
    ```
    - `codex-session:` label: BOLD
    - `where:` / `why:` / `hint:` / `caused by:` labels: BOLD
    - Path in `where:` line: as-is (no color)
    - Hint text: as-is (no color, already actionable)

11. **Warnings & Prompts (stderr)** — Standard patterns:
    - Deprecation warnings: `"warning: {message}"` — BOLD_YELLOW prefix `warning:`, rest plain
    - Auth warnings: multi-line, BOLD_YELLOW `warning:` prefix on first line
    - Interactive prompts: `"remove account 'X' permanently? [y/N]: "` — no color (prompt text)
    - Runtime narration: `"[codex-session] {message}"` — DIM `[codex-session]` prefix, rest plain
    - All stderr messages respect `--quiet` (suppress non-errors) and `--silent` (suppress all)

12. **JSON Output** — When `--format json` is used:
    - Pretty-printed JSON (`serde_json::to_writer_pretty`)
    - No ANSI escape codes, ever
    - All timestamps as unix epoch integers (not human-readable)
    - snake_case or kebab-case field names matching the `#[serde(rename_all = "kebab-case")]`
      convention already used
    - Single JSON object per command (not JSONL — that's upstream codex's `--json`)

13. **`--format` Flag Convention** — The wrapper uses `--format <text|json>` (via `OutputFormat`
    enum), never `--json`. Reason: upstream `codex` uses `--json` for JSONL event streaming; using
    the same flag name in the wrapper would cause confusion and potential passthrough collision.
    Every wrapper-owned read command must accept `--format`.

14. **Per-Command Output Specifications** — A table or subsection for each command showing its
    target output layout. This is the detailed reference for rounds 02 and 03.

    **account list:**
    ```text
      ACCOUNT    AUTH   LAST USED      STATUS
    ▸ cwnt       ✓      2m ago         active (lru)
      default    ✓      1d 14h ago
    ```

    **account current:**
    ```text
    ▸ cwnt (lru)
    ```

    **account health (table mode):**
    ```text
    RANK  SCORE   ACCOUNT      TOKEN   PLAN            STATUS       ACTIVE  COOLDOWN  FETCHED
    1     12.50   cwnt         ok      pro             live         true    false     2m ago
    —     0.00    default      invalid unknown         cache_only   false   true      1d 3h ago
    ```
    With colors: header DIM, token ok→BOLD_GREEN / invalid→BOLD_RED / unknown→BOLD_YELLOW,
    status live→BOLD_GREEN / cache_only→BOLD_YELLOW / cache_missing→BOLD_RED,
    active true→BOLD_CYAN, cooldown true→BOLD_RED, fetched→DIM, active account name→BOLD.

    **account cooldown show:**
    ```text
    ACCOUNT     STATUS       RESETS         REASON
    cwnt        eligible     —              —
    default     cooled-down  1h 23m         RateLimit429
    ```
    With colors: header DIM, eligible→BOLD_GREEN, cooled-down→BOLD_RED, reset countdown→BOLD_YELLOW,
    account name→BOLD.

    **account mutations (add/use/remove/refresh):**
    ```text
    ✓ account added: cwnt
      path: /home/gu/.local/state/codex-session/accounts/cwnt
    ```
    With colors: `✓ account {verb}:` → BOLD_GREEN, name → BOLD, path → DIM.

    **doctor:**
    ```text
    account:         cwnt
    account-source:  lru
    ...

    STATUS   CHECK                      DETAIL
    OK       config_recipe.active             default (source: config.default)
    WARN     account.cooldowns          1 account(s) in cooldown: default
    FAIL     child.binary               could not find `codex` on PATH=...

    Next:
      - child.binary: set CODEX_SESSION_CHILD_BIN or install `codex` on PATH

    summary: 8 OK, 1 WARN, 1 FAIL
    ```
    With colors: header DIM, OK→BOLD_GREEN, WARN→BOLD_YELLOW, FAIL→BOLD_RED,
    check names→BOLD, summary counts colored to match their status.

    **config status:**
    Key-value pairs with labels in DIM, values plain. Boolean values colored
    (true→BOLD_GREEN, false→DIM). Paths in DIM. Layer sub-items indented.

    **config_recipe list:**
    ```text
    CONFIG_RECIPE    LAYERS   VALID   MANIFEST
    default    3        ✓       /path/to/default.yaml
    work       2        ✗       /path/to/work.yaml
    ```

    **config_recipe show:**
    Key-value pairs similar to config status.

    **version:**
    ```text
    codex-session 0.1.0
    codex /usr/local/bin/codex 1.0.2
    account:         cwnt (source: lru)
    ```
    With colors: version numbers→BOLD, account info→same as account current.

### Step 3: Update CLAUDE.md

Add two sections to `/workspaces/codex-session/CLAUDE.md`:

**Section: CLI Design System**
A short paragraph pointing to `docs/design/cli-style-guide.md` as the authoritative reference for
all user-facing output. Tell agents to consult it before writing any `write_*` method, error
rendering, or stderr message.

**Section: Breaking changes policy**
State that codex-session is pre-v1.0, breaking changes are expected and desired, and no
compatibility layers should ever be added. Specifically note: use `--format json` (never `--json`),
and avoid overlapping with native `codex` CLI flags.

## Acceptance Criteria

- [ ] `docs/design/cli-style-guide.md` exists and contains all 14 sections listed above
- [ ] The document uses only ANSI-safe colors (the 8 basic colors via `anstyle::AnsiColor`)
- [ ] Every wrapper-owned command has a target output specification in the document
- [ ] The two-channel model (user-facing vs logs) is clearly defined with rules for what goes where
- [ ] `CLAUDE.md` has a pointer to the design system and the pre-v1.0 policy
- [ ] No code changes were made (only docs)
- [ ] `just lint` passes (no markdown lint issues that would break CI)

## Next Round

Round 02 will rename `quota_styles` → `styles`, add the `GREEN` constant, and apply the design
system to all account commands: list, current, health, cooldown show, and mutations (add/use/remove/
refresh). It will also migrate the cooldown `--json` flag to `--format json`.
