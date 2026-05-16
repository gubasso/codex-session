#!/usr/bin/env bash
# shellcheck shell=bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMPDIR="$(mktemp -d /tmp/codex-session-smoke.XXXXXX)"
trap 'rm -rf "$TMPDIR"' EXIT

HOME_DIR="$TMPDIR/home"
CACHE_DIR="$TMPDIR/cache"
BIN_DIR="$TMPDIR/bin"
mkdir -p "$HOME_DIR/.codex" "$CACHE_DIR" "$BIN_DIR"

cat >"$BIN_DIR/codex" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$#" >"$TMPDIR_ARGC"
printf '' >"$TMPDIR_ARGV"
for arg in "$@"; do
  printf '%s\n' "$arg" >>"$TMPDIR_ARGV"
done
exit 0
EOF
chmod +x "$BIN_DIR/codex"

TMPDIR_ARGC="$TMPDIR/codex.argc"
TMPDIR_ARGV="$TMPDIR/codex.argv"
export TMPDIR_ARGC TMPDIR_ARGV

HOME="$HOME_DIR" XDG_CACHE_HOME="$CACHE_DIR" PATH="$BIN_DIR:/usr/bin:/bin" \
  bash "$ROOT/bin/codex-session" self help >/dev/null

HOME="$HOME_DIR" XDG_CACHE_HOME="$CACHE_DIR" PATH="$BIN_DIR:/usr/bin:/bin" \
  bash "$ROOT/bin/codex-session" --help >/dev/null
[ "$(cat "$TMPDIR_ARGC")" = "1" ]
[ "$(cat "$TMPDIR_ARGV")" = "--help" ]
[ ! -e "$HOME_DIR/.codex/config.toml" ]
[ ! -e "$CACHE_DIR/codex-session/last-merge" ]

cp "$ROOT/tests/fixtures/base.toml" "$HOME_DIR/.codex/config.base.toml"
cp "$ROOT/tests/fixtures/target-with-local.toml" "$HOME_DIR/.codex/config.toml"
rm -f "$CACHE_DIR/codex-session/last-merge"

HOME="$HOME_DIR" XDG_CACHE_HOME="$CACHE_DIR" PATH="$BIN_DIR:/usr/bin:/bin" \
  bash "$ROOT/bin/codex-session" exec >/dev/null

diff -u "$ROOT/tests/fixtures/expected-merged.toml" "$HOME_DIR/.codex/config.toml"
