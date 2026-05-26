#!/usr/bin/env bash
set -euo pipefail
# Simulates `codex login` by writing auth.json into $CODEX_HOME.
# Used by integration tests to verify CODEX_HOME isolation.
if [ -z "${CODEX_HOME-}" ]; then
  echo "CODEX_HOME not set" >&2
  exit 1
fi
case "${1-}" in
  login)
    mkdir -p "$CODEX_HOME"
    cat > "$CODEX_HOME/auth.json" <<'JSON'
{"tokens":{"access_token":"fake-at","refresh_token":"fake-rt","account_id":"fake-acct"}}
JSON
    chmod 600 "$CODEX_HOME/auth.json"
    echo "Successfully logged in"
    ;;
  logout)
    # No-op — simulates codex logout in an empty dir
    ;;
  *)
    echo "unexpected arg: ${1-}" >&2
    exit 1
    ;;
esac
