# shellcheck shell=bash
__cmd_self_help() {
  cat <<'EOF'
codex-session — wrapper around `codex` that keeps ~/.codex/config.toml in sync
with the stow-managed ~/.codex/config.base.toml while preserving machine-local
sections (e.g. [projects."..."]).

Usage:
  codex-session [<codex args>...]      # pass-through to real `codex`
  codex-session self <verb> [args]     # wrapper-only commands

Wrapper verbs:
  self help            Show this message.
  self version         Print codex-session wrapper version.
  self config-status   Print base/target/stamp paths and whether a merge is needed.
  self config-merge    Force re-merge regardless of stamp freshness.
  self show-local      Print machine-local TOML sections that would be preserved.

Anything not under `self` is forwarded verbatim to the real `codex` binary,
including `--help`, `--version`, `exec`, `resume`, `login`, and any future verbs.
EOF
}
