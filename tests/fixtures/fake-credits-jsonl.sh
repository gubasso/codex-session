#!/usr/bin/env bash
set -euo pipefail

# Credit-exhaustion shape observed live on codex 0.135.0 (2026-06-03), see
# docs/upstream-codex.md §F9: an `error` event followed by `turn.failed`
# whose error object carries ONLY a message — no error_code, no
# http_status_code, no retry_after.
printf '%s\n' '{"type":"error","message":"Your workspace is out of credits. Add credits to continue."}'
printf '%s\n' '{"type":"turn.failed","message":"Your workspace is out of credits. Add credits to continue.","error":{"message":"Your workspace is out of credits. Add credits to continue."}}'
printf 'marker:codex-session-fake-credits home=%s\n' "${CODEX_HOME:-unset}" >&2
exit 1
