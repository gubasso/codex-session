# Upstream codex behavior reference

Canonical reference for how upstream [`openai/codex`](https://github.com/openai/codex)
behaves, used as the source of truth for any change in `codex-session` that
depends on codex's config, auth, trust, or process semantics. Don't guess —
consult or update this file.

- **Last verified:** 2026-06-03
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

## F6b — `--profile` CLI flag (v0.134+ contract)

Codex supports `-p, --profile <CONFIG_PROFILE>` to select a named profile at
runtime. Since v0.134.0 the flag overlays the file
`$CODEX_HOME/<profile>.config.toml` on top of the base `$CODEX_HOME/config.toml`.
Per-profile files contain **bare top-level keys** — there is no
`[profiles.<name>]` header. The base `config.toml` must contain no top-level
`profile = "..."` selector and no `[profiles.*]` table. There is no in-file
default selector; `--profile` is the only way to activate one.

Model resolution precedence (highest to lowest): CLI `--model` →
`-c model=` override → `--profile`-overlaid file → top-level `config.toml` →
catalog default.

- **Sources:** [Advanced Configuration §Profiles](https://developers.openai.com/codex/config-advanced#profiles),
  [CLI reference (`--profile`)](https://developers.openai.com/codex/cli/reference),
  [openai/codex release v0.134.0](https://github.com/openai/codex/releases/tag/rust-v0.134.0),
  `codex --help` (verified 2026-05-28).
  `Last verified`: 2026-05-28.
- **Implementation note (round-04 contract):**
  The heartbeat probe in `src/services/account/gate.rs` calls codex with
  `--profile ping` and an isolated `CODEX_HOME`. The probe `CODEX_HOME` contains
  a base `config.toml` plus a sibling `ping.config.toml` copied 1:1 from
  `profiles/ping.config.toml`. codex-session itself never injects `--profile`
  for user-facing exec calls.

## F6c — Legacy profile form rejection (v0.134+ breaking change)

Codex v0.134.0 stopped accepting two legacy forms inside `$CODEX_HOME/config.toml`:

1. The top-level selector `profile = "<name>"`.
2. The nested table `[profiles.<name>]`.

Either form triggers a hard error with migration guidance pointing at
<https://developers.openai.com/codex/config-advanced#profiles>: users must
remove the legacy selector/table from `config.toml` and migrate those
settings into a sibling `<name>.config.toml` file with bare top-level keys.
There is NO backward-compat flag or environment variable to re-enable the
legacy form.

This is a load-bearing fact for `codex-session`: the composer must (a) reject
the legacy form at the input layer (`configs/*.toml`), (b) never emit it in
`$CODEX_HOME/config.toml`. See §F6b for the new contract and
[CLAUDE.md § Codex config compatibility](../CLAUDE.md) for the wrapper rule.

- **Sources:** [Codex changelog](https://developers.openai.com/codex/changelog)
  (v0.134.0 entry — profile-restructuring PR group, individual PR IDs not
  re-verified at write time; consult the changelog page or
  `git log --grep=profile` in `openai/codex` for the exact PRs),
  [release v0.134.0](https://github.com/openai/codex/releases/tag/rust-v0.134.0).
  `Last verified`: 2026-05-28.

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

## F9 — codex 0.135.0 `exec --json` JSONL schema

Verified against the `codex` binary reported by `codex-session --version`
(`codex 0.135.0` at the time of capture). The `exec --json` stream is JSONL
with one event per line. The wrapper logic in `src/services/account/codex_events.rs`
and `src/services/account/failover.rs` currently depends on these facts:

- Event types observed in exec mode: `thread.started`, `turn.started`,
  `turn.completed`, `turn.failed`, `item.*`, and `token_count`.
- `token_count` is **not guaranteed**: a successful exec run may emit no
  `token_count` event at all, with usage carried only on
  `turn.completed.usage` (`input_tokens`, `cached_input_tokens`,
  `output_tokens`, `reasoning_output_tokens`). Verified live on
  codex 0.135.0 (2026-06-03). The live schema test accepts a
  `turn.completed`-with-`usage` stream as intact.
- `turn.failed` may also be represented by a top-level `error` object in the
  line payload; the wrapper should keep tolerating both shapes.
- `token_count.rate_limits` maps cleanly onto the wrapper's
  `RateLimitSnapshot` / `RateLimitWindow` model. Observed fields:
  `primary`, `secondary`, `used_percent`, `window_minutes`,
  `resets_in_seconds`, `resets_at`, `plan_type`, and
  `rate_limit_reached_type`.
- The rate-limit classifier currently consumes three reset sources, in this
  order: `turn.failed.error.retry_after`, the relevant
  `token_count.rate_limits.*.resets_in_seconds` window, then free text like
  `try again in N`.
- Error discriminants observed and depended on today:
  `usage_limit_reached`, `usage_limit_exceeded`, and
  `context_window_exceeded`.
- Credit exhaustion has its own shape (observed live 2026-06-03 on
  codex 0.135.0): an `error` event followed by `turn.failed` whose
  `error` object carries only a `message` ("Your workspace is out of
  credits. Add credits to continue.") — no `error_code`, no
  `http_status_code`. The wrapper classifies this as
  `Category::CreditExhausted` (message-based, case-insensitive
  "out of credits"), writes a cooldown that expires at the exhausted
  quota window's reset (`retry::credit_cooldown`, derived from the
  account's own usage data; 300s fallback when quota is unavailable),
  then rotates on the auto path or returns `ResumeBlocked` on the
  resume path.
- Credit semantics (verified live 2026-06-03 against `wham/usage`, and
  corroborated by [`openai/codex#19830`](https://github.com/openai/codex/issues/19830)
  whose error text offers "purchase more credits OR try again at
  [reset time]"): credits are a **workspace-level overflow pool**
  consumed only after the plan's included windows are exhausted; they
  refill via manual/auto top-up only (no scheduled reset). The
  "out of credits" error fires when a window is at 100% used AND the
  workspace has no credits — the account recovers at the window reset
  **without** a top-up. Upstream's `RateLimitReachedType`
  (`codex-rs/protocol/src/protocol.rs`) has both
  `WorkspaceOwnerCreditsDepleted` and `WorkspaceMemberCreditsDepleted`
  variants, and `RateLimitSnapshot` carries an optional
  `CreditsSnapshot { has_credits, unlimited, balance }`.
- The raw `wham/usage` response (the endpoint
  `src/services/account/quota.rs` polls) carries more than the
  windows the wrapper parses today — verified fields: `rate_limit.allowed`,
  `rate_limit.limit_reached`, per-window `used_percent` /
  `limit_window_seconds` / `reset_after_seconds` / `reset_at`,
  `credits { has_credits, unlimited, overage_limit_reached, balance, … }`,
  `spend_control`, and `rate_limit_reached_type { type, details }`
  (observed `"workspace_owner_credits_depleted"`). The wrapper currently
  retains only the windows; the credit fields are a candidate for
  `account quota`/`health` surfacing.
- Bare `429` failures still happen without a `usage_limit_*` code. In that
  case the wrapper must inspect the snapshot: high `used_percent` means
  window exhaustion; healthy headroom means a transient limit.

Open question: [`openai/codex#14728`](https://github.com/openai/codex/issues/14728)
tracks whether `rate_limits` is always populated in exec mode. Round 1's live
capture test in [tests/codex_exec_json_live.rs](../tests/codex_exec_json_live.rs)
does not assert this either way — it records a verdict of `populated` / `null` /
`absent-from-token_count` / `no-token_count-event` and tolerates all of them
(including a rate-limited run that emits no events). So exec-mode `rate_limits`
population is observed-and-recorded, not guaranteed. Because of that the
classifier never relies on the snapshot alone: it falls back to `retry_after`
and `"try again in N"` text parsing when the snapshot is missing or null. F9
must be re-verified before changing the policy.

## codex-session-specific notes

These don't fit a single F# above but are load-bearing for keeping the
wrapper aligned with codex.

- **Cache layer is the trust persistence home.** `codex-session` writes
  trust decisions to `<XDG_CACHE_HOME>/codex-session/configs.toml`, which
  is loaded first by `compose()` (`src/services/config_recipe/mod.rs`). The
  stow-managed user layers in `<XDG_CONFIG_HOME>/codex-session/configs/`
  and profile override files in `<XDG_CONFIG_HOME>/codex-session/profiles/`
  (including `profiles/*.config.toml`) are **never** modified by
  the wrapper — that's the composeability contract.
- **Replay requires an active config-recipe.** `compose()` is the only producer
  that reads the cache layer. Stock-mode invocations (no config-recipe) still
  _write_ trust to the cache (the post-flight sync runs unconditionally),
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
endpoint before deleting local auth. Fail-closed: if revocation fails,
local auth is preserved so the user can retry.

- **Sources:** [PR #17825 — "Revoke ChatGPT tokens on logout"](https://github.com/openai/codex/pull/17825).

## F12 — `CODEX_HOME` fully scopes auth

All auth operations read/write `$CODEX_HOME/auth.json`. When
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

Returns new `access_token` + new `refresh_token` (rotation). The old
`refresh_token` is permanently invalidated after a single use. Reusing
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
`$CODEX_HOME/sessions/YYYY/MM/DD/rollout-*.jsonl`. A separate
`$CODEX_HOME/session_index.jsonl` indexes sessions for the picker.
`CODEX_HOME` fully scopes session storage (see F6); there are no
cross-`CODEX_HOME` lookups.

Thread IDs are client-generated UUIDs (currently v7 via `UUID::now_v7()`
in `Session::new`). The format is an implementation detail — callers
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
  session's thread ID to the originating account **and group-id**. On
  `resume`, the wrapper looks up the thread ID (or resolves `--last`) from
  this index, using the stored account and group-id to select the correct
  `CODEX_HOME` before forwarding to codex. This means `exec resume <ID>`
  works across terminals and PIDs because both account and group-id are
  persisted in `thread-index.jsonl`. When the index has no hit for
  `resume <ID>`, the wrapper now scans the registered accounts' rollout
  stores for a matching `rollout-*.jsonl`; if it finds one, it pins the
  resume back to that owner, emits a `warning:`, and backfills the missing
  thread-index entry. If no registered account owns the rollout, the
  wrapper returns a classified `ResumeOwnerMissing` error with recent-thread
  candidates instead of auto-selecting another account. `--last` and
  `--all` remain index-only and return `ResumeIndexEmpty` when there is no
  recorded thread to resume. See F15 for the constraint that sandbox flags
  must match between original and resumed calls.

## F15 — Sandbox mode mismatch on resume

`exec resume` fails with JSON-RPC -32600 ("no rollout found") when the
sandbox mode of the resumed call differs from the sandbox mode of the
original session. The OpenAI backend validates that session parameters
match on resume and rejects requests with incompatible sandbox changes.

Observed failure chain:

1. Stage 1 creates a thread with `--sandbox read-only`.
2. Stage 3 attempts `exec resume <thread-id>` with `--full-auto`
   (or `--sandbox workspace-write`).
3. Backend returns -32600 "no rollout found" despite the thread
   existing in the local index and the local rollout file being present.

The local `codex-session` wrapper correctly resolves the thread ID and
routes to the right account/group via `thread-index.jsonl` (or a bounded
rollout-store recovery scan on an index miss). A post-pin `-32600`
therefore means one of two things:

- the local rollout still exists, and the backend rejected the resume due
  to a sandbox mismatch (`ResumeNoRollout` / `SandboxMismatch`);
- or the local rollout is absent/deleted (`ResumeNoRollout` /
  `RolloutMissing`).

Workaround: use `--dangerously-bypass-approvals-and-sandbox` uniformly
across all stages that share a thread. This flag bypasses bubblewrap
entirely and sends no sandbox parameters to the backend, so there is no
mismatch to validate.

- **Sources:** Empirical testing (2026-05-27),
  [issue #3947 — "Agent cannot edit files using sandbox when resuming"](https://github.com/openai/codex/issues/3947),
  [issue #5322 — "Sandbox flags not honored on resume"](https://github.com/openai/codex/issues/5322),
  [issue #16994 — "No rollout materializes"](https://github.com/openai/codex/issues/16994),
  [issue #18676 — "Resume session: stream disconnected"](https://github.com/openai/codex/issues/18676),
  [issue #19661 — "exec resume fails with encrypted_content"](https://github.com/openai/codex/issues/19661),
  [issue #23875 — "Desktop drops approvals_reviewer after resume"](https://github.com/openai/codex/issues/23875).
- **Implementation note:** `codex-session` does not intercept or translate
  sandbox flags — they pass through to the codex binary unchanged. What the
  wrapper does add is post-exec classification: after a pinned resume
  returns `-32600` / "no rollout found", it checks whether the local
  rollout still exists and turns the raw backend failure into either a
  sandbox-mismatch hint or a deleted-rollout hint.

## F16 — Cross-account thread resume is not possible in stock codex

A thread/rollout created while authenticated as account **A** **cannot** be
resumed under a different account **B** using stock codex. Resumption is
bound to the originating account at two independent layers — either alone is
sufficient to make a true cross-account resume fail.

**Layer 1 — Filesystem (fully `CODEX_HOME`-scoped).**
Rollouts live at `$CODEX_HOME/sessions/YYYY/MM/DD/rollout-*.jsonl` and
discovery never crosses `CODEX_HOME` boundaries (see F6, F14). Account A's
rollout under `…/accounts/A/groups/<g>/sessions/…` is simply not visible to
a resume run with account B's `CODEX_HOME`. Stock `codex resume <id>` from B
fails to locate any rollout.

**Layer 2 — Server-side account/provider binding.**
Even if the rollout file were made reachable, the OpenAI backend ties a
conversation to the provider/account that created it. Discovery and
continuation default to filtering by the _active_ provider/account, so a
mismatched auth token loses the original context. This is the same family
of validation that produces the -32600 "no rollout found" response in F15.

**Consequence / failure signature.**
Because both layers reject it, any path that lets a resume run under a
_different_ account than the one that owns the thread reproduces the
"no rollout found" failure — the canonical example being `--account auto`
re-resolving to a different account between the stage that _created_ the
thread and the stage that _resumes_ it.

**How `codex-session` avoids it (it does not actually resume cross-account).**
The wrapper's job on resume is to pin back to the **owning** account, never
to cross accounts. It uses the out-of-band `thread-index.jsonl`
(`<state_dir>/thread-index.jsonl`) to map thread ID → originating
account + group-id, then sets `CODEX_HOME` to that account's directory
before forwarding to codex (see F14). If the thread-index entry is missing,
the wrapper can recover the owner from the registered accounts' rollout
stores (`AccountResolutionSource::RolloutScan`), but that is still an
owner-discovery step, not cross-account resume. Stock codex therefore
always resumes _as the owning account, within a single `CODEX_HOME`_ and
never sees a cross-account request. Corollary: the wrapper must keep the
resume pinned to the owner's account — if the resume re-resolves the
account (e.g. `--account auto`, or a fallback path that ignores both the
index hit and rollout-scan recovery), both layers above will reject it.
For the same reason, an explicit `--account` pin that disagrees with the
resolved owner is **overridden** on resume: the wrapper emits a `warning:`
naming the owner and proceeds pinned to it rather than forwarding a resume
that both layers would reject.

- **Sources:** Investigation 2026-05-29 (this repo:
  `src/commands/pass_through.rs:434-586` `resolve_resume_account`,
  `src/commands/pass_through.rs:587-667` `run_resume`,
  `src/services/session/thread_index.rs:83-89` `lookup`; cross-refs F6, F14, F15),
  [issue #20004 — "Preserve local history visibility/continuation across providers/accounts"](https://github.com/openai/codex/issues/20004),
  [issue #15494 — "Switching model_provider hides existing local sessions from resume/fork/history"](https://github.com/openai/codex/issues/15494),
  [docs: features](https://developers.openai.com/codex/cli/features),
  [DeepWiki: Session Resumption and Forking](https://deepwiki.com/openai/codex/4.4-session-resumption-and-forking).
- **Implementation note:** This is a hard upstream constraint, not a wrapper
  limitation. Do not attempt to add a cross-account resume shim; the correct
  design is to always resolve a resume back to the thread's owning account via
  `thread-index.jsonl`.

## Sources (full list)

- Docs: <https://developers.openai.com/codex/local-config/>,
  <https://developers.openai.com/codex/config-reference>,
  <https://developers.openai.com/codex/cli/features>,
  <https://developers.openai.com/codex/noninteractive>
- Issues: [#3947](https://github.com/openai/codex/issues/3947),
  [#4407](https://github.com/openai/codex/issues/4407),
  [#4432](https://github.com/openai/codex/issues/4432),
  [#4940](https://github.com/openai/codex/issues/4940),
  [#5322](https://github.com/openai/codex/issues/5322),
  [#9695](https://github.com/openai/codex/issues/9695),
  [#9696](https://github.com/openai/codex/issues/9696),
  [#10332](https://github.com/openai/codex/issues/10332),
  [#10347](https://github.com/openai/codex/issues/10347),
  [#10389](https://github.com/openai/codex/issues/10389),
  [#13242](https://github.com/openai/codex/issues/13242),
  [#14547](https://github.com/openai/codex/issues/14547),
  [#15433](https://github.com/openai/codex/issues/15433),
  [#15494](https://github.com/openai/codex/issues/15494),
  [#15538](https://github.com/openai/codex/issues/15538),
  [#15767](https://github.com/openai/codex/issues/15767),
  [#16994](https://github.com/openai/codex/issues/16994),
  [#18065](https://github.com/openai/codex/issues/18065),
  [#18483](https://github.com/openai/codex/issues/18483),
  [#18676](https://github.com/openai/codex/issues/18676),
  [#18771](https://github.com/openai/codex/issues/18771),
  [#19661](https://github.com/openai/codex/issues/19661),
  [#20004](https://github.com/openai/codex/issues/20004),
  [#21196](https://github.com/openai/codex/issues/21196),
  [#23875](https://github.com/openai/codex/issues/23875)
- Discussions: [#1076](https://github.com/openai/codex/discussions/1076)
- PRs: [#14718](https://github.com/openai/codex/pull/14718),
  [#14849](https://github.com/openai/codex/pull/14849),
  [#17595](https://github.com/openai/codex/pull/17595),
  [#17825](https://github.com/openai/codex/pull/17825),
  [#18626](https://github.com/openai/codex/pull/18626),
  [#20667](https://github.com/openai/codex/pull/20667),
  [#21747](https://github.com/openai/codex/pull/21747)
