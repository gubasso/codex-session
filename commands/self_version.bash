# shellcheck shell=bash
__cmd_self_version() {
  printf '%s %s\n' "$CODEX_SESSION_NAME" "$(__read_version)"
}
