# shellcheck shell=bash
# shellcheck disable=SC2034  # consumed by tests/codex-session.bats via `load`

TEST_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CODEX_SESSION_BIN="$TEST_ROOT/bin/codex-session"
FIXTURES_DIR="$TEST_ROOT/tests/fixtures"

setup() {
  export HOME="$BATS_TEST_TMPDIR/home"
  export XDG_CACHE_HOME="$BATS_TEST_TMPDIR/cache"
  export PATH="/usr/bin:/bin"
  mkdir -p "$HOME/.codex" "$XDG_CACHE_HOME"
}

make_fake_codex() {
  local bindir="$BATS_TEST_TMPDIR/bin"
  mkdir -p "$bindir"
  cat >"$bindir/codex" <<EOF
#!/usr/bin/env bash
printf '%s\n' "\$#" >"$BATS_TEST_TMPDIR/codex.argc"
printf '' >"$BATS_TEST_TMPDIR/codex.argv"
for arg in "\$@"; do
  printf '%s\n' "\$arg" >>"$BATS_TEST_TMPDIR/codex.argv"
done
exit 0
EOF
  chmod +x "$bindir/codex"
  export PATH="$bindir:$PATH"
}

install_fixture() {
  local name="$1"
  case "$name" in
    base.toml)
      cp "$FIXTURES_DIR/base.toml" "$HOME/.codex/config.base.toml"
      ;;
    target-with-local.toml)
      cp "$FIXTURES_DIR/target-with-local.toml" "$HOME/.codex/config.toml"
      ;;
    expected-merged.toml)
      cp "$FIXTURES_DIR/expected-merged.toml" "$HOME/.codex/config.toml"
      ;;
    *)
      printf 'unknown fixture: %s\n' "$name" >&2
      return 1
      ;;
  esac
}

# Portable mtime setters: GNU touch supports `-d <iso-string>`, BSD/macOS
# touch supports `-t [[CC]YY]MMDDhhmm[.SS]`. Probe once and dispatch.
__touch_set_mtime() {
  local when="$1" target="$2"
  # when is "YYYY MM DD hh mm ss" space-separated for portability across formats.
  local parts
  read -r -a parts <<<"$when"
  local Y="${parts[0]}" M="${parts[1]}" D="${parts[2]}" h="${parts[3]}" m="${parts[4]}" s="${parts[5]}"
  if touch -d "${Y}-${M}-${D}T${h}:${m}:${s}Z" "$target" 2>/dev/null; then
    return 0
  fi
  touch -t "${Y}${M}${D}${h}${m}.${s}" "$target"
}

touch_older() {
  __touch_set_mtime "2020 01 01 00 00 00" "$1"
}

touch_newer() {
  __touch_set_mtime "2030 01 01 00 00 00" "$1"
}

# Portable mtime reader: GNU stat uses `-c %Y`, BSD/macOS stat uses `-f %m`.
mtime_of() {
  stat -c %Y "$1" 2>/dev/null || stat -f %m "$1"
}
