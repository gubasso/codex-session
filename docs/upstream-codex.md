# Upstream codex behavior reference

Canonical reference for how upstream [`openai/codex`](https://github.com/openai/codex)
behaves, used as the source of truth for any change in `codex-session` that
depends on codex's config, auth, trust, or process semantics. Don't guess —
consult or update this file.

- **Last verified:** 2026-05-27
- **Codex version SoT:** `codex-session --version` (prints child binary path + version).
- **Maintenance:** if this file looks stale (codex has released several
  versions since `Last verified`), re-run the **Re-verification recipe**
  below and the spot-check greps before relying on these facts. Update the
  date line when you do.

## Re-verification recipe

Use this to confirm trust write semantics on a current codex:

```bash
mkdir -p /tmp/codex-trust-probe
CODEX_HOME=/tmp/codex-trust-probe codex   # trust the cwd interactively, then exit
find /tmp/codex-trust-probe -type f -print0 | xargs -0 ls -la
grep -R "trust_level" /tmp/codex-trust-probe
```

The trust entry should land in `/tmp/codex-trust-probe/config.toml` under a
`[projects."<canonical-cwd>"]` table. If it doesn't, F1/F2 below have
shifted and the trust-sync wiring in `src/services/trust_sync.rs` (and the
session-config diff source in `src/commands/pass_through.rs`) needs to be
updated to match.

## F1 — Trust write target

Codex writes trust decisions to **`$CODEX_HOME/config.toml`** (not a sidecar
file). The path is fully parameterized on `CODEX_HOME`; there is no
hard-coded fallback to `~/.codex/config.toml` in the write path.

- **Sources:** [PR #17595 — "Do not fail thread start when trust persistence fails"](https://github.com/openai/codex/pull/17595),
  [docs: local-config](https://developers.openai.com/codex/local-config/),
  [issue #15433 — "Separate project trust state from `~/.codex/config.toml`"](https://github.com/openai/codex/issues/15433).
- **Implementation note:** `codex-session` spawns codex with
  `CODEX_HOME=<session-dir>` (`src/commands/pass_through.rs`), so codex's
  trust writes land in `<session-dir>/config.toml`. The post-flight
  trust sync reads that path and merges new entries into the
  machine-local cache layer.

## F2 — Trust schema

```toml
[projects."<absolute-path>"]
trust_level = "trusted"   # or "untrusted"
```

Two-value enum today (`"trusted"` | `"untrusted"`). The vocabulary belongs
to codex; `codex-session` relays verbatim and does **not** filter or
interpret `trust_level`.

- **Sources:** [docs: config-reference](https://developers.openai.com/codex/config-reference),
  [PR #18626 — "Respect explicit untrusted project config"](https://github.com/openai/codex/pull/18626).

## F3 — Path key canonicalization

Codex canonicalizes path keys via [`dunce::canonicalize`](https://docs.rs/dunce/)
before writing and re-canonicalizes the cwd before lookup, so a symlinked
cwd maps to the same entry as its realpath. On Windows the canonical form
may include a UNC prefix (`\\?\C:\…`).

- **Sources:** [issue #18483 — "Desktop thread list flips between symlink and realpath"](https://github.com/openai/codex/issues/18483),
  [PR #14849 — "canonicalize symlinked Linux sandbox cwd"](https://github.com/openai/codex/pull/14849),
  [issue #10347 — "Windows UNC paths treated as canonical"](https://github.com/openai/codex/issues/10347).
- **Implementation note:** `codex-session` preserves the codex-written form
  **verbatim** into the cache layer. We do **not** re-canonicalize on our
  side — codex re-canonicalizes the cwd on every launch, so the
  canonical-form key it wrote is the same key it looks up. If a user
  hand-authors `[projects."<path>"]` entries in a stow-managed layer, the
  user is responsible for using the canonical path themselves.

## F4 — Decline persistence

Selecting "No" at the trust prompt writes `trust_level = "untrusted"` to
the persisted config. Codex respects that decision on subsequent launches
(does not re-prompt). Untrusted projects also have project-scoped config
layers (`.codex/config.toml`) and `.codex/skills` loading disabled.

- **Sources:** [issue #9696 — "Clarify that selecting 'No' also disables project skills"](https://github.com/openai/codex/issues/9696),
  [issue #4940 — "Don't Trust blocks all writes"](https://github.com/openai/codex/issues/4940),
  [PR #18626](https://github.com/openai/codex/pull/18626).
- **Implementation note:** `codex-session` persists both `"trusted"` and
  `"untrusted"` entries to the cache layer. The trust_sync diff filters by
  value equality against the baseline, not by `trust_level` content — so a
  future codex release that introduces a third state will continue to
  round-trip without code changes.

## F5 — Prompt gating

The trust prompt is gated on `should_show_trust_screen` in
[`codex-rs/tui/src/lib.rs`](https://github.com/openai/codex/tree/main/codex-rs/tui/src).
The prompt is suppressed when:

- a global `approval_policy` or `sandbox_mode` is set,
- yolo mode / `--dangerously-bypass-approvals-and-sandbox` is active,
- or the project already has an explicit trust entry.

In any of those modes no `[projects.…]` write fires.

- **Sources:** [issue #14547 — "Trust prompt appears on every launch despite yolo"](https://github.com/openai/codex/issues/14547),
  [issue #9695 — "YOLO suppresses trust prompt but still blocks .codex/skills"](https://github.com/openai/codex/issues/9695).
- **Implementation note:** trust_sync naturally no-ops when no
  `[projects.…]` was written (`TrustSyncOutcome::Unchanged`). No special
  case is needed.

## F6 — `CODEX_HOME` semantics

`CODEX_HOME` defaults to `~/.codex` and **redirects all codex state** when
set: config (`config.toml`), auth (`auth.json`), logs, MCP servers, thread
history. There are no hard-coded fallbacks to `~/.codex/*`.

- **Sources:** [docs: local-config](https://developers.openai.com/codex/local-config/),
  [PR #20667 — "Load configured environments from CODEX_HOME"](https://github.com/openai/codex/pull/20667),
  [issue #18065 — "Misleading `~/.codex/config.toml` references ignore $CODEX_HOME"](https://github.com/openai/codex/issues/18065),
  [issue #4407 — "Change the hardcoded $HOME/.codex path"](https://github.com/openai/codex/issues/4407).

## F6b — `--profile` CLI flag

Codex supports `-p, --profile <CONFIG_PROFILE>` to select a named
configuration profile at runtime. The flag maps to a `[profiles.<name>]`
section in `config.toml`. Model resolution precedence (highest to lowest):
CLI `--model` → `-c model=` override → `--profile` section → top-level
`model` → catalog default.

- **Sources:** `codex --help` (verified 2026-05-26).
- **Implementation note:** The heartbeat probe in
  `src/services/account/gate.rs` uses `--profile ping` with an isolated
  `CODEX_HOME` to select the probe model without `--model` hardcoding.

## F7 — Write-back model

Codex uses `toml_edit` to modify `config.toml`, which preserves user
formatting and comments when possible, paired with an atomic `tempfile +
rename` write. Trust-persistence failures are log-and-swallow — codex does
not abort the session on a write error.

- **Sources:** [PR #17595](https://github.com/openai/codex/pull/17595),
  `codex-rs/core/src/config.rs` in the upstream tree.
- **Implementation note:** `codex-session`'s trust_sync parser is AST-based
  via the `toml` crate; it tolerates `toml_edit`-flavored output without
  caring about whitespace, comments, or table-style differences. The
  caller in `src/commands/pass_through.rs::persist_trust` also
  log-and-swallows, matching upstream's contract.

## F8 — Stale path hazard

Codex canonicalizes **all** configured project paths at startup. If any
entry points at a path that has become inaccessible (unmounted drive,
deleted directory, network share offline), codex hangs during startup.

- **Sources:** [issue #18771 — "Unrelated project in config.toml with inaccessible directory causes immediate hang"](https://github.com/openai/codex/issues/18771).
- **Implementation note:** `codex-session` v1 adds entries only; it never
  prunes stale entries. A future `codex-session doctor --prune-stale-trust`
  command should walk the cache layer's `[projects.*]` table and remove
  entries whose path is no longer accessible. Until then, accumulated
  stale entries are a known operational hazard.

## codex-session-specific notes

These don't fit a single F# above but are load-bearing for keeping the
wrapper aligned with codex.

- **Cache layer is the trust persistence home.** `codex-session` writes
  trust decisions to `<XDG_CACHE_HOME>/codex-session/settings.toml`, which
  is loaded first by `compose()` (`src/services/config_recipe/mod.rs`). The
  stow-managed user layers in `<XDG_CONFIG_HOME>/codex-session/settings/`
  are **never** modified by the wrapper — that's the composeability
  contract.
- **Replay requires an active config-recipe.** `compose()` is the only producer
  that reads the cache layer. Stock-mode invocations (no config-recipe) still
  *write* trust to the cache (the post-flight sync runs unconditionally),
  but they do not replay it on the next launch — codex re-prompts. Users
  who want trust persistence should ensure a config-recipe is active (typically
  by having a `default.yaml` manifest in `config-recipes/`).
- **Hardened-write parity with `auth.json`.** All cache-layer writes go
  through `src/services/auth.rs` primitives (`with_lock`,
  `secure_file_read`, `secure_file_write_atomic`, `ensure_owned_dir_0700`)
  — `O_NOFOLLOW`, ownership/mode checks, flock serialization, 0o600 atomic
  rename. Same threat model and same operational shape as codex's own
  auth file.

## F10 — Token revocation on login

`codex login` revokes any previously-stored managed ChatGPT token via
`POST https://auth.openai.com/oauth/revoke` before saving the new one.
If revocation fails, login still succeeds (graceful failure).

- **Sources:** [PR #21747 — "Revoke superseded auth tokens on relogin"](https://github.com/openai/codex/pull/21747).

## F11 — Token revocation on logout

`codex logout` sends the stored `refresh_token` to the revocation
endpoint before deleting local auth.  Fail-closed: if revocation fails,
local auth is preserved so the user can retry.

- **Sources:** [PR #17825 — "Revoke ChatGPT tokens on logout"](https://github.com/openai/codex/pull/17825).

## F12 — `CODEX_HOME` fully scopes auth

All auth operations read/write `$CODEX_HOME/auth.json`.  When
`CODEX_HOME` is set, codex does not touch `~/.codex/auth.json`.

Keyring entries (when `cli_auth_credentials_store` is `keyring` or
`auto`) are keyed by a hash of the `CODEX_HOME` path — different
`CODEX_HOME` values maintain completely isolated credential stores.

- **Sources:** [docs: local-config](https://developers.openai.com/codex/local-config/),
  [Codex Auth docs](https://developers.openai.com/codex/auth).

## F13 — Token refresh endpoint

OAuth refresh at `POST https://auth.openai.com/oauth/token` with:

```json
{
  "grant_type": "refresh_token",
  "client_id": "app_EMoamEEZ73f0CkXaXp7hrann",
  "refresh_token": "<stored_rt>"
}
```

Returns new `access_token` + new `refresh_token` (rotation).  The old
`refresh_token` is permanently invalidated after a single use.  Reusing
it returns `refresh_token_reused`.

- **Sources:** [OpenAI Apps SDK Auth](https://developers.openai.com/apps-sdk/build/auth),
  [Issue #10332 — "Race condition in OAuth token refresh"](https://github.com/openai/codex/issues/10332),
  [Issue #4432 — "First-class multi-account auth"](https://github.com/openai/codex/issues/4432).

## F14 — Thread/session resume

`codex resume` reopens an earlier session by ID or with convenience flags:

- `codex resume` — interactive picker of recent sessions.
- `codex resume <SESSION_ID>` — target a specific session.
- `codex resume --last` — most recent session from current working directory.
- `codex resume --all` — picker across all directories (not cwd-scoped).
- `codex exec resume <SESSION_ID> [PROMPT]` — non-interactive resume.
- `codex exec resume --last [PROMPT]` — non-interactive, most recent.

Session transcripts are stored as date-sharded JSONL rollout files under
`$CODEX_HOME/sessions/YYYY/MM/DD/rollout-*.jsonl`.  A separate
`$CODEX_HOME/session_index.jsonl` indexes sessions for the picker.
`CODEX_HOME` fully scopes session storage (see F6); there are no
cross-`CODEX_HOME` lookups.

Thread IDs are client-generated UUIDs (currently v7 via `UUID::now_v7()`
in `Session::new`).  The format is an implementation detail — callers
should treat the ID as an opaque string.

- **Sources:** [docs: features](https://developers.openai.com/codex/cli/features),
  [docs: CLI reference](https://developers.openai.com/codex/cli/reference),
  [docs: non-interactive mode](https://developers.openai.com/codex/noninteractive),
  [issue #15538 — "Ephemeral resume"](https://github.com/openai/codex/issues/15538),
  [issue #15767 — "Support custom session ID for new threads"](https://github.com/openai/codex/issues/15767),
  [issue #13242 — "Feature request: --session-id flag"](https://github.com/openai/codex/issues/13242),
  [issue #19661 — "Resume fails with encrypted_content"](https://github.com/openai/codex/issues/19661),
  [issue #21196 — "Missing rollout files"](https://github.com/openai/codex/issues/21196),
  [discussion #1076 — "Resuming a previous session"](https://github.com/openai/codex/discussions/1076).
- **Implementation note:** `codex-session` maintains a cross-account
  `thread-index.jsonl` at `<state_dir>/thread-index.jsonl` that maps each
  session's thread ID to the originating account **and group-id**.  On
  `resume`, the wrapper looks up the thread ID (or resolves `--last`) from
  this index, using the stored account and group-id to select the correct
  `CODEX_HOME` before forwarding to codex.  This means `exec resume <ID>`
  works across terminals and PIDs because both account and group-id are
  persisted in `thread-index.jsonl`.  When the index has no hit, the
  wrapper falls back to normal account resolution and forwards the resume
  command as-is.

## Sources (full list)

- Docs: <https://developers.openai.com/codex/local-config/>,
  <https://developers.openai.com/codex/config-reference>,
  <https://developers.openai.com/codex/cli/features>,
  <https://developers.openai.com/codex/noninteractive>
- Issues: [#4407](https://github.com/openai/codex/issues/4407),
  [#4432](https://github.com/openai/codex/issues/4432),
  [#4940](https://github.com/openai/codex/issues/4940),
  [#9695](https://github.com/openai/codex/issues/9695),
  [#9696](https://github.com/openai/codex/issues/9696),
  [#10332](https://github.com/openai/codex/issues/10332),
  [#10347](https://github.com/openai/codex/issues/10347),
  [#10389](https://github.com/openai/codex/issues/10389),
  [#13242](https://github.com/openai/codex/issues/13242),
  [#14547](https://github.com/openai/codex/issues/14547),
  [#15433](https://github.com/openai/codex/issues/15433),
  [#15538](https://github.com/openai/codex/issues/15538),
  [#15767](https://github.com/openai/codex/issues/15767),
  [#18065](https://github.com/openai/codex/issues/18065),
  [#18483](https://github.com/openai/codex/issues/18483),
  [#18771](https://github.com/openai/codex/issues/18771),
  [#19661](https://github.com/openai/codex/issues/19661),
  [#21196](https://github.com/openai/codex/issues/21196)
- Discussions: [#1076](https://github.com/openai/codex/discussions/1076)
- PRs: [#14718](https://github.com/openai/codex/pull/14718),
  [#14849](https://github.com/openai/codex/pull/14849),
  [#17595](https://github.com/openai/codex/pull/17595),
  [#17825](https://github.com/openai/codex/pull/17825),
  [#18626](https://github.com/openai/codex/pull/18626),
  [#20667](https://github.com/openai/codex/pull/20667),
  [#21747](https://github.com/openai/codex/pull/21747)
