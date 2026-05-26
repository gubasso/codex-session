#!/usr/bin/env bash
set -euo pipefail
{
  printf 'HTTP 401 Unauthorized\n'
  printf 'marker:codex-session-fake-401 home=%s\n' "${CODEX_HOME:-unset}"
} >&2
exit 1
