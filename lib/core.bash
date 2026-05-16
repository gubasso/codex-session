# shellcheck shell=bash
# shellcheck disable=SC2034  # globals consumed by sibling sourced files
CODEX_SESSION_NAME="codex-session"
CODEX_SESSION_VERSION_FILE="${CODEX_SESSION_ROOT}/VERSION"

CODEX_DIR="${HOME}/.codex"
BASE="${CODEX_DIR}/config.base.toml"
TARGET="${CODEX_DIR}/config.toml"
CACHE_DIR="${XDG_CACHE_HOME:-$HOME/.cache}/codex-session"
STAMP="${CACHE_DIR}/last-merge"

__read_version() {
  if [[ -r "$CODEX_SESSION_VERSION_FILE" ]]; then
    head -n1 "$CODEX_SESSION_VERSION_FILE"
  else
    printf 'unknown\n'
  fi
}
