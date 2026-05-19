#!/usr/bin/env bash
set -euo pipefail
# Baseline keys we always probe (the wrapper-private set tested by name).
for key in CODEX_SESSION_CHILD_BIN CODEX_SESSION_LOG_FILE CODEX_SESSION_LOG_DIR CODEX_SESSION_REENTRY; do
  printf '%s=%s\n' "$key" "${!key-}"
done
for key in CODEX_HOME HELLO; do
  printf '%s=%s\n' "$key" "${!key-}"
done
# Plus any other CODEX_SESSION_* keys actually present in the child env,
# so tests can assert the whole namespace is scrubbed (not just the
# baseline). One `name=value` line per env entry.
while IFS='=' read -r key _value; do
  case "$key" in
    CODEX_SESSION_CHILD_BIN|CODEX_SESSION_LOG_FILE|CODEX_SESSION_LOG_DIR|CODEX_SESSION_REENTRY)
      ;; # already printed above
    CODEX_SESSION_*)
      printf 'LEAKED:%s=%s\n' "$key" "${!key-}"
      ;;
  esac
done < <(env)
