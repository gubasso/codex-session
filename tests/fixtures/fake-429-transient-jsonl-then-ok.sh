#!/usr/bin/env bash
set -euo pipefail

sentinel="${CODEX_HOME:-unset}/transient-429-once"
if [ ! -f "$sentinel" ]; then
  touch "$sentinel"
  printf '%s\n' '{"type":"token_count","rate_limits":{"primary":{"used_percent":42.0,"resets_in_seconds":7},"rate_limit_reached_type":"primary"}}'
  printf '%s\n' '{"type":"turn.failed","message":"HTTP 429 Too Many Requests","error":{"retry_after":0,"http_status_code":429}}'
  printf 'marker:codex-session-transient-then-ok home=%s stage=first\n' "${CODEX_HOME:-unset}" >&2
  exit 1
fi

printf 'marker:codex-session-transient-then-ok home=%s stage=second\n' "${CODEX_HOME:-unset}" >&2
printf 'transient cleared\n'
exit 0
