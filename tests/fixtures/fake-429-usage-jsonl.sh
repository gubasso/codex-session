#!/usr/bin/env bash
set -euo pipefail

printf '%s\n' '{"type":"token_count","rate_limits":{"primary":{"used_percent":100.0,"resets_in_seconds":88},"rate_limit_reached_type":"primary"}}'
printf '%s\n' '{"type":"turn.failed","message":"usage limit hit","error":{"error_code":"usage_limit_reached","retry_after":44,"http_status_code":429}}'
printf 'marker:codex-session-fake-429-usage home=%s\n' "${CODEX_HOME:-unset}" >&2
exit 1
