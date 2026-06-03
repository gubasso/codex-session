#!/usr/bin/env bash
set -euo pipefail

printf '%s\n' '{"type":"token_count","rate_limits":{"primary":{"used_percent":42.0,"resets_in_seconds":7},"rate_limit_reached_type":"primary"}}'
printf '%s\n' '{"type":"turn.failed","message":"HTTP 429 Too Many Requests","error":{"retry_after":0,"http_status_code":429}}'
printf 'marker:codex-session-fake-429-transient-always home=%s\n' "${CODEX_HOME:-unset}" >&2
exit 1
