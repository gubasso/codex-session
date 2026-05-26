#!/usr/bin/env bash
set -euo pipefail
STATE_FILE="${CODEX_HOME}/.fake-401-state"
if [ ! -f "$STATE_FILE" ]; then
  touch "$STATE_FILE"
  {
    printf 'HTTP 401 Unauthorized\n'
    printf 'marker:codex-session-fake-401-then-ok home=%s\n' "${CODEX_HOME:-unset}"
  } >&2
  exit 1
fi
printf 'marker:codex-session-ok home=%s\n' "${CODEX_HOME:-unset}" >&2
exit 0
