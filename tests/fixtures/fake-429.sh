#!/usr/bin/env bash
set -euo pipefail
{
  printf 'HTTP 429 Too Many Requests\n'
  printf 'marker:codex-session-fake-429 home=%s\n' "${CODEX_HOME:-unset}"
} >&2
exit 1
