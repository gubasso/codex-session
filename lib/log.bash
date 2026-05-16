# shellcheck shell=bash
__log_err()  { printf '%s\n' "$*" >&2; }
__die()      { __log_err "$*"; exit 1; }
__die_codex_missing() { __die "ERROR: codex binary not found in PATH"; }
