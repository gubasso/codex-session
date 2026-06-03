#!/usr/bin/env bash
set -euo pipefail

printf '%s\n' '{"type":"turn.failed","message":"context window exceeded for request","error":{"error_code":"context_window_exceeded","http_status_code":400}}'
printf 'marker:codex-session-fake-unhandled home=%s\n' "${CODEX_HOME:-unset}" >&2
exit 1
