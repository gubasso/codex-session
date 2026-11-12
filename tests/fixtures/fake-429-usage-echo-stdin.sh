#!/usr/bin/env bash
set -euo pipefail

# Reads the whole prompt from stdin, as `codex exec -` does, then hits the
# usage limit, so the wrapper rotates and runs this script again.
prompt="$(cat)"
printf '%s\n' '{"type":"turn.failed","message":"usage limit hit","error":{"error_code":"usage_limit_reached","retry_after":44,"http_status_code":429}}'
printf 'marker:codex-session-fake-stdin home=%s prompt=[%s]\n' "${CODEX_HOME:-unset}" "$prompt" >&2
exit 1
