# CLI Style Guide

This document is the authoritative target design for wrapper-owned
codex-session output. Some details intentionally describe future Round 02/03
behavior where current code differs.

## 1. Principles

- Scannable at a glance.
- Color is semantic, not decorative.
- Colorless output remains complete, with no invisible text or broken
  alignment.
- Two-channel separation: stdout for results, stderr for everything else.
- JSON and logs are for machines.

## 2. Output Channels

codex-session has two output channels:

- User-facing output: stdout is for command results. Stderr is for warnings,
  prompts, errors, and progress narration. User-facing text may use semantic
  ANSI styling, human-readable times, status indicators, and Unicode symbols.
- Structured logs: `tracing` writes to a file sink that is always JSON and has
  no ANSI. The optional stderr mirror is still machine-oriented and respects
  `stderr_color()` when rendered in pretty mode.

Routing rules:

- Command results go to stdout.
- Warnings, prompts, errors, and progress narration go to stderr.
- Debug and trace details go to logs only.
- Pass-through child output is owned by upstream `codex`; the wrapper must not
  restyle it.

## 3. Color Palette

Use only the 8 basic ANSI colors through `anstyle::AnsiColor`. Do not use RGB,
256-color, bright ANSI variants, or background colors.

| Constant      | ANSI        | Semantic meaning                              |
| ------------- | ----------- | --------------------------------------------- |
| `BOLD`        | bold        | Entity names, emphasis                        |
| `DIM`         | dimmed      | Headers, metadata, secondary info, paths      |
| `BOLD_CYAN`   | bold+cyan   | Active/current item, selection indicator      |
| `GREEN`       | green       | Checkmarks (`✓`), positive indicators         |
| `BOLD_GREEN`  | bold+green  | OK status, eligible, positive values, success |
| `BOLD_YELLOW` | bold+yellow | Warning, unknown, pending, countdown values   |
| `RED`         | red         | Error markers (`✗`), negative indicators      |
| `BOLD_RED`    | bold+red    | Fail status, cooldown active, critical errors |

`GREEN` is for symbols and checkmarks only. `BOLD_GREEN` is for text labels and
values.

## 4. Color Gating

All user-facing styling must use the color policy module (`color::should_color`):

1. `NO_COLOR` non-empty disables color absolutely.
2. `FORCE_COLOR` or `CLICOLOR_FORCE` truthy, meaning non-empty and non-zero,
   enables color even for non-TTY output.
3. Non-TTY output disables color unless forced.
4. `CLICOLOR=0` disables color on TTY output.
5. TTY output enables color by default.

Every renderer must call `color::should_color(Stream)` for the destination
stream and then emit styles with `style_open()` and `style_close()`. When
`use_color` is false, the same output must remain readable and aligned.

## 5. Typography & Symbols

- `▸` (U+25B8): current or active marker, followed by one space, styled
  `BOLD_CYAN`.
- `✓` (U+2713): positive, authenticated, or OK indicator, styled `GREEN` or
  `BOLD_GREEN` until `GREEN` exists.
- `⚠` (U+26A0): warning or caution indicator, styled `BOLD_YELLOW`.
- `✗` (U+2717): negative, no-auth, or fail indicator, styled `RED`.
- `—` (U+2014): missing or not applicable value, preferably styled `DIM`.

Do not use box drawing for tables.

## 6. Table Layout

- Header rows use ALL CAPS and `DIM`.
- Column widths are calculated from visible data with sensible minimum widths.
- Text columns are left-aligned.
- Numeric columns are right-aligned.
- The active/current row uses a `▸` prefix.
- Output has no trailing whitespace.
- ANSI escape sequences are stripped before calculating visible widths.

## 7. Status Indicators

| Value pattern                | Style         | Example context                |
| ---------------------------- | ------------- | ------------------------------ |
| `ok` / `valid` / `live`      | `BOLD_GREEN`  | token status, health status    |
| `warn` / `unknown` / `cache` | `BOLD_YELLOW` | token unknown, `cache_only`    |
| `fail` / `invalid` / `error` | `BOLD_RED`    | token invalid, `cache_missing` |
| `active` / `current`         | `BOLD_CYAN`   | active account, current marker |
| `eligible`                   | `BOLD_GREEN`  | cooldown eligible              |
| `cooled-down`                | `BOLD_RED`    | cooldown active                |

Boolean styling is context-dependent:

- `active: true` uses `BOLD_CYAN`.
- `cooldown: true` uses `BOLD_RED`.
- `has_auth: true` uses a `GREEN` checkmark.
- Generic `true` uses `BOLD_GREEN` and generic `false` uses `DIM`, only for
  neutral config fields.

## 8. Time Formatting

Text mode never shows raw Unix timestamps. Use `human_age()` for past times and
`human_duration_until()` for future reset times.

`human_duration_secs()` uses:

- `>=1d`: `{d}d {h}h`
- `>=1h`: `{h}h {m}m`
- `>=1m`: `{m}m {s}s`
- `<1m`: `{s} s`

Missing values render as `—` or `never` in `DIM`, depending on command context.
Current implementation note: `human_duration_secs(0)` returns `0 s`, not
`never`. The `never` rendering is a semantic rule for callers to apply when the
underlying value is absent or semantically null; it is not a change to
`human_duration_secs()` itself.

## 9. Progress Bars

Quota percentage displays use a 20-column progress bar:

- Filled cells: `█`
- Empty cells: `░`
- `>50%`: `BOLD_GREEN`
- `>20%`: `BOLD_YELLOW`
- `<=20%`: `BOLD_RED`

The 20-column bar widget is used only for quota percentage displays; animated
spinners are specified in §9b.

## 9b. Spinners & live progress narration

Spinners are used for wrapper-owned wait-time narration where the command is
performing live work and stdout still belongs to the final command result.

Current scope:

- `account health`, only when not using `--fast`.
- `account quota`.
- `doctor`, as rolling single-line progress.
- `account refresh` and `account add`, before and after interactive login, never
  during the native login flow.

Spinner output is stderr only. Command results, including text tables and JSON,
stay on stdout.

Visible spinners are suppressed when any of these are true:

- stderr is not a TTY.
- `--format json` is selected.
- `--quiet` or `--silent` is selected.
- `account health --fast` is selected.
- The effective stderr `tracing` mirror is not `Off`, such as with `-v`,
  `--log-stderr`, or `log.mirror_stderr = true`.

Color gating is not visibility gating. `NO_COLOR` on a TTY produces a plain but
visible spinner; non-TTY stderr suppresses the spinner entirely.

Spinner frames use `{spinner:.cyan} {msg}` when stderr color is enabled and
`{spinner} {msg}` when plain. Color is decided through
`color::should_color(Stream::Stderr)`. Spinners use
`enable_steady_tick(80ms)`.

Completion markers render as `✓ <msg>` in `GREEN` for success and `✗ <msg>` in
`RED` for failure. When color is disabled, use `[ok] <msg>` and `[err] <msg>`
ASCII fallbacks. Finished spinner lines render as marker plus message only; the
spinner style must switch to `{msg}` before finishing. Transient spinners may
finish-and-clear when no residual progress line is useful.

While a spinner group is live, any other wrapper stderr writer, including
warnings and prompts, must write through the spinner suspend mechanism. The
stderr `tracing` mirror writes directly to `std::io::stderr` and can emit from
inside spawned tasks, so this round disables visible spinners whenever the
effective stderr mirror is not `Off`. A future progress-aware tracing writer may
relax that restriction.

Message conventions:

- In-progress messages use a present participle, such as `Checking account
  "work"...`.
- Account names are quoted.
- Finish messages are 60 characters or fewer.

## 10. Error Rendering

`error.rs::render()` uses this shape:

```text
codex-session: {what}
  where: {path}
  why:   {why_line}
  hint:  {hint}
  caused by: {chain}
```

The labels use these styles: `codex-session:` `BOLD_RED`, `where:` `BOLD`,
`why:` `BOLD`, `hint:` `BOLD_CYAN`, and `caused by:` `BOLD`. Path and hint
text remain plain. Usage errors may be delegated to clap, which renders its
own ANSI. The `where:` and `hint:` lines are optional. `caused by:` repeats for
the error chain.

## 11. Warnings & Prompts

- Warnings use a `warning:` prefix in `BOLD_YELLOW`; the message body is plain.
- Auth warnings may be multi-line.
- Account rotation/switch warnings are stderr warnings. They are shown by
  default and suppressed by `--quiet` or `--silent`.
- Prompts such as `remove account 'X' permanently? [y/N]:` are uncolored.
- Runtime narration uses a dim `[codex-session]` prefix and a plain message
  body.
- Target behavior: `--quiet` suppresses non-error stderr. `--silent`
  suppresses all wrapper stderr, including errors.

Current implementation note: `--quiet` currently only suppresses the stderr log
mirror. It is not yet wired to `narrate()` or `write_warning()`; that is a
later-round code change.

## 12. JSON Output

When `--format json` is used:

- Use `serde_json::to_writer_pretty`.
- Never emit ANSI escape codes.
- Timestamps are Unix epoch integers.
- Emit one JSON object or array per command, not JSONL.
- Field naming follows the `#[serde(rename_all = "kebab-case")]` convention
  already used across view structs.

## 13. `--format` Flag Convention

Wrapper read commands use `--format <text|json>` through the `OutputFormat`
enum. Do not introduce wrapper `--json` flags because upstream
`codex --json` means JSONL event streaming.

## 14. Per-Command Output Specifications

These are target layouts for wrapper-owned output surfaces. Current code may
differ until later implementation rounds.

Supported output formats per command:

| Command                      | `--format text`        | `--format json` |
| ---------------------------- | ---------------------- | --------------- |
| `version` / `--version`      | yes                    | yes             |
| `completion`                 | raw stdout passthrough | no              |
| `config status`              | yes                    | yes             |
| `config-recipe list`         | yes                    | yes             |
| `config-recipe show`         | yes                    | yes             |
| `config-recipe compose`      | yes (text only)        | no              |
| `doctor`                     | yes                    | yes             |
| `account list`               | yes                    | yes             |
| `account current`            | yes                    | yes             |
| `account add/remove/refresh` | yes                    | yes             |
| `account quota`              | yes                    | yes             |
| `account health`             | yes                    | yes             |
| `account cooldown show`      | yes                    | yes             |
| `account cooldown clear`     | yes                    | yes             |
| `login` / `logout`           | stderr narration       | no              |
| `--dry-run`                  | yes (text only)        | no              |

### `version` and Global `--version`

```text
codex-session 0.1.0
codex /usr/local/bin/codex 1.0.2
account:         cwnt (source: auto)
```

Version numbers use `BOLD`. Account info uses the same account styling as
`account current`. The target account line may differ from the current
implementation.

### `completion`

`completion <shell>` writes the generated shell-completion script directly to
stdout. The wrapper does not add color, headers, summaries, or stderr narration.

```text
# shell-specific completion script emitted by clap_complete
```

### `config status`

Key-value pairs use `DIM` labels and plain values. Boolean values are colored
for neutral config fields: `true` uses `BOLD_GREEN`, `false` uses `DIM`. Paths
use `DIM`. Layer sub-items are indented.

```text
active-config-recipe: default
manifest-path:        /path/to/default.yaml
account:              cwnt
account-source:       auto
group-id:             project
group-id-source:      config.project
codex_home:           /home/gu/.codex
accounts:             2 (1 in cooldown)
active-auth:          true
session-root:         /home/gu/.local/state/codex-session/sessions
session-source:       default
child-bin:            /usr/local/bin/codex
layers:
  base => /path/to/base.toml (exists=true)
  local => /path/to/local.toml (exists=false)
log.file:             true
log.verbose:          false
log.mirror-stderr:    true
log.format:           json
log.stderr-format:    auto
sources:
  defaults
  user:    /home/gu/.config/codex-session/config.toml
  project: none
  env:     false
  cli:     false
```

### `config-recipe list`

```text
CONFIG_RECIPE    LAYERS   VALID   MANIFEST
default    3        ✓       /path/to/default.yaml
work       2        ✗       /path/to/work.yaml
```

Header uses `DIM`. `✓` uses `GREEN`; `✗` uses `RED`. Manifest paths use `DIM`.

### `config-recipe show`

Key-value pairs follow `config status`: labels use `DIM`, values are plain,
paths use `DIM`, and layer sub-items are indented.

```text
config-recipe: default
manifest:      /path/to/default.yaml
layers:
  base => /path/to/base.toml (exists=true)
  local => /path/to/local.toml (exists=false)
```

### `config-recipe compose`

`config-recipe compose` has text output only.

```text
config-recipe: default
group-id:      project
session-dir:   /path/to/session
config:        /path/to/session/config.toml
sidecar:       /path/to/session/codex-session.json
session-meta:  /path/to/session/session-meta.json
```

Labels use `DIM`. Paths use `DIM`.

### `doctor`

```text
account:         cwnt
account-source:  auto
...

ENVIRONMENT
✓  xdg.paths             XDG_CONFIG_HOME=/path/to/config XDG_CACHE_HOME=/path/to/cache ...
✗  child.binary          could not find `codex` on PATH=...

CONFIG RECIPE
✓  config-recipe.active  default (source: config.default)
⚠  account.cooldowns     1 account(s) in cooldown: default

Next:
  - child.binary: set CODEX_SESSION_CHILD_BIN or install `codex` on PATH

summary: 8 OK, 1 WARN, 1 FAIL
```

Section titles and the summary label use ALL-CAPS `DIM`. Check names use
`BOLD`. Doctor check rows use `✓` in `BOLD_GREEN`, `⚠` in `BOLD_YELLOW`, and
`✗` in `BOLD_RED`. Summary counts are colored to match their status.

### `account list`

```text
  ACCOUNT    AUTH   LAST USED      STATUS
▸ cwnt       ✓      2m ago         active (auto)
  default    ✓      1d 14h ago
```

Header uses `DIM`. The active row uses `▸` in `BOLD_CYAN`. Authenticated
accounts use `✓` in `GREEN`; unauthenticated accounts use `✗` in `RED`. Missing
last-used values use `—` in `DIM`.

### `account current`

```text
▸ cwnt (auto)
```

The marker uses `BOLD_CYAN`; the account name uses `BOLD`; the `({source})`
suffix uses `DIM` (secondary metadata).

### `account add/use/remove/refresh`

```text
✓ account added: cwnt
  path: /home/gu/.local/state/codex-session/accounts/cwnt
```

`✓ account {verb}:` uses `BOLD_GREEN`, the account name uses `BOLD`, and the
path uses `DIM`. The verb is `added`, `selected`, `removed`, or `refreshed`.

### `account quota`

OAuth quota output:

```text
#1 12.50 cwnt (active)
5-hour      ███████████████░░░░░  75% left   resets in 1h 23m
Weekly      ██████████░░░░░░░░░░  50% left   resets in 2d 4h
2m ago, live
```

API-key quota output:

```text
cwnt (api-key)
Quota not available (API-key auth)
2m ago, live
```

Error quota output:

```text
cwnt
Error: failed to fetch quota
2m ago, live
```

Account names use `BOLD`. Rank and score metadata use `DIM`. Active markers use
`BOLD_CYAN`. Progress bars use the quota thresholds from Section 9. Error text
uses `RED`.

Pool totals:

```text
#1 12.50 cwnt (active)
5-hour      ███████████████░░░░░  75% left   resets in 1h 23m
Weekly      ██████████░░░░░░░░░░  50% left   resets in 2d 4h
2m ago, live

#2 11.20 alice
5-hour      ████████████████░░░░  80% left   resets in 4h 12m
Weekly      ██████████░░░░░░░░░░  50% left   resets in 2d 4h
2m ago, live

TOTAL (avg across 2 accounts)
5-hour      ███████████████░░░░░  78%
Weekly      ██████████░░░░░░░░░░  50%
```

The TOTAL panel renders only when at least 2 OAuth accounts contributed. Reset
countdowns are omitted on aggregate rows by design because resets are
per-account. The TOTAL header uses `DIM`, and aggregate bars use the same
`percent_style` thresholds from Section 9 as the per-account rows. Percentages
render as whole numbers to match the per-account quota rows.

### `account health`

Table mode:

```text
RANK  SCORE   ACCOUNT      TOKEN   PLAN            STATUS       ACTIVE  COOLDOWN  FETCHED
1     12.50   cwnt         ok      pro             live         true    false     2m ago
—     0.00    default      invalid unknown         cache_only   false   true      1d 3h ago
```

Header uses `DIM`. Token `ok` uses `BOLD_GREEN`, `invalid` uses `BOLD_RED`, and
`unknown` uses `BOLD_YELLOW`. Status `live` uses `BOLD_GREEN`, `cache_only` uses
`BOLD_YELLOW`, and `cache_missing` uses `BOLD_RED`. `active=true` uses
`BOLD_CYAN`, `cooldown=true` uses `BOLD_RED`, fetched age uses `DIM`, and the
active account name uses `BOLD`.

Verbose mode uses key-value blocks with the same status, boolean, and time
styling rules:

```text
account: cwnt
rank: 1
score: 12.50
plan: pro
token: ok
token_detail: valid (expires 2026-06-01)
status: live
active: true
cooldown: false
last_used: 2m ago
fetched: 2m ago

account: default
rank: —
score: 0.00
plan: unknown
token: invalid
token_detail: expired
status: cache_only
active: false
cooldown: true
last_used: —
fetched: 1d 3h ago
```

Account names use `BOLD`. Token, status, active, and cooldown values follow the
same color rules as table mode. `rank: —` uses `DIM`.

### `account cooldown show`

```text
ACCOUNT     STATUS       RESETS         REASON
cwnt        eligible     —              —
default     cooled-down  1h 23m         RateLimit429
```

Header uses `DIM`. `eligible` uses `BOLD_GREEN`, `cooled-down` uses `BOLD_RED`,
reset countdowns use `BOLD_YELLOW`, and account names use `BOLD`.

### `account cooldown clear`

Clearing a single account:

```text
✓ cooldown cleared: cwnt
```

Clearing all accounts:

```text
✓ cooldowns cleared: 2
```

Success prefixes use `BOLD_GREEN`. Account names and counts use `BOLD`.

### Managed `login`/`logout`

Managed login and logout write runtime narration to stderr, not stdout.

```text
[codex-session] verifying token for account 'cwnt' (source: auto)...
[codex-session] account 'cwnt' is already authenticated.
```

```text
[codex-session] logging out account 'cwnt' (source: auto)...
[codex-session] revoking token via isolated codex logout...
[codex-session] account 'cwnt' is now logged out.
```

The `[codex-session]` prefix uses `DIM`; the message body is plain. Any native
`codex login` or `codex logout` child output is owned by upstream `codex`.

### `--dry-run`

`--dry-run` writes a deterministic invocation report to stdout and does not
style or summarize it.

```text
account: cwnt
account-source: auto
binary: /usr/local/bin/codex
argv:
  [0] exec
  [1] hello
env.inherit: true
env.remove:
  CODEX_HOME
env.set:
  CODEX_HOME=/path/to/session
```
