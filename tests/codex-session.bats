#!/usr/bin/env bats

load ./test_helper.bash

@test "self help exits 0 and prints Usage:" {
  run bash "$CODEX_SESSION_BIN" self help
  [ "$status" -eq 0 ]
  [[ "$output" == *"Usage:"* ]]
}

@test "self version exits 0 and prints codex-session 0.1.0" {
  run bash "$CODEX_SESSION_BIN" self version
  [ "$status" -eq 0 ]
  [ "$output" = "codex-session 0.1.0" ]
}

@test "--help is passed through to codex" {
  make_fake_codex
  run bash "$CODEX_SESSION_BIN" --help
  [ "$status" -eq 0 ]
  [ "$(cat "$BATS_TEST_TMPDIR/codex.argc")" = "1" ]
  [ "$(cat "$BATS_TEST_TMPDIR/codex.argv")" = "--help" ]
}

@test "--version is passed through to codex" {
  make_fake_codex
  run bash "$CODEX_SESSION_BIN" --version
  [ "$status" -eq 0 ]
  [ "$(cat "$BATS_TEST_TMPDIR/codex.argc")" = "1" ]
  [ "$(cat "$BATS_TEST_TMPDIR/codex.argv")" = "--version" ]
}

@test "exec foo bar is passed through to codex" {
  make_fake_codex
  run bash "$CODEX_SESSION_BIN" exec foo bar
  [ "$status" -eq 0 ]
  [ "$(cat "$BATS_TEST_TMPDIR/codex.argc")" = "3" ]
  mapfile -t args <"$BATS_TEST_TMPDIR/codex.argv"
  [ "${args[0]}" = "exec" ]
  [ "${args[1]}" = "foo" ]
  [ "${args[2]}" = "bar" ]
}

@test "no-arg invocation calls codex with zero argv" {
  make_fake_codex
  run bash "$CODEX_SESSION_BIN"
  [ "$status" -eq 0 ]
  [ "$(cat "$BATS_TEST_TMPDIR/codex.argc")" = "0" ]
  [ ! -s "$BATS_TEST_TMPDIR/codex.argv" ]
}

@test "missing codex on PATH errors exactly" {
  run bash "$CODEX_SESSION_BIN"
  [ "$status" -eq 1 ]
  [ "$output" = "ERROR: codex binary not found in PATH" ]
  [ "${#lines[@]}" -eq 1 ]
}

@test "missing BASE falls through to codex without creating TARGET or STAMP" {
  make_fake_codex
  run bash "$CODEX_SESSION_BIN" exec
  [ "$status" -eq 0 ]
  [ "$(cat "$BATS_TEST_TMPDIR/codex.argv")" = "exec" ]
  [ ! -e "$HOME/.codex/config.toml" ]
  [ ! -e "$XDG_CACHE_HOME/codex-session/last-merge" ]
}

@test "fresh stamp skips merge and leaves TARGET mtime unchanged" {
  make_fake_codex
  install_fixture base.toml
  install_fixture target-with-local.toml
  mkdir -p "$XDG_CACHE_HOME/codex-session"
  : >"$XDG_CACHE_HOME/codex-session/last-merge"
  touch_older "$HOME/.codex/config.base.toml"
  touch_newer "$XDG_CACHE_HOME/codex-session/last-merge"
  local before
  before="$(mtime_of "$HOME/.codex/config.toml")"
  run bash "$CODEX_SESSION_BIN" exec
  [ "$status" -eq 0 ]
  [ "$(mtime_of "$HOME/.codex/config.toml")" = "$before" ]
}

@test "missing STAMP triggers merge to expected output" {
  make_fake_codex
  install_fixture base.toml
  install_fixture target-with-local.toml
  run bash "$CODEX_SESSION_BIN" exec
  [ "$status" -eq 0 ]
  diff -u "$FIXTURES_DIR/expected-merged.toml" "$HOME/.codex/config.toml"
}

@test "BASE newer than STAMP triggers merge" {
  make_fake_codex
  install_fixture base.toml
  install_fixture target-with-local.toml
  mkdir -p "$XDG_CACHE_HOME/codex-session"
  : >"$XDG_CACHE_HOME/codex-session/last-merge"
  touch_older "$XDG_CACHE_HOME/codex-session/last-merge"
  touch_newer "$HOME/.codex/config.base.toml"
  run bash "$CODEX_SESSION_BIN" exec
  [ "$status" -eq 0 ]
  diff -u "$FIXTURES_DIR/expected-merged.toml" "$HOME/.codex/config.toml"
}

@test "missing TARGET with existing BASE writes BASE only" {
  make_fake_codex
  install_fixture base.toml
  run bash "$CODEX_SESSION_BIN" exec
  [ "$status" -eq 0 ]
  diff -u "$FIXTURES_DIR/base.toml" "$HOME/.codex/config.toml"
  # shellcheck disable=SC2314  # plain `!` is intentional; bats 1.5+ `run !` not required here
  ! grep -q 'Machine-local' "$HOME/.codex/config.toml"
}

@test "self config-status prints booleans and needs_merge" {
  install_fixture base.toml
  run bash "$CODEX_SESSION_BIN" self config-status
  [ "$status" -eq 0 ]
  [[ "$output" == *"base:        $HOME/.codex/config.base.toml (exists=true)"* ]]
  [[ "$output" == *"target:      $HOME/.codex/config.toml (exists=false)"* ]]
  [[ "$output" == *"stamp:       $XDG_CACHE_HOME/codex-session/last-merge (exists=false)"* ]]
  [[ "$output" == *"needs_merge: yes"* ]]
}

@test "self show-local prints only local project sections" {
  install_fixture base.toml
  install_fixture target-with-local.toml
  run bash "$CODEX_SESSION_BIN" self show-local
  [ "$status" -eq 0 ]
  [ "$output" = "$(cat <<'EOF'
[projects."/tmp/example"]
trust_level = "trusted"

[projects."/tmp/other"]
trust_level = "untrusted"
EOF
)" ]
}

@test "self config-merge forces rewrite when stamp is fresh" {
  install_fixture base.toml
  install_fixture target-with-local.toml
  mkdir -p "$XDG_CACHE_HOME/codex-session"
  : >"$XDG_CACHE_HOME/codex-session/last-merge"
  touch_older "$HOME/.codex/config.base.toml"
  touch_newer "$XDG_CACHE_HOME/codex-session/last-merge"
  run bash "$CODEX_SESSION_BIN" self config-merge
  [ "$status" -eq 0 ]
  diff -u "$FIXTURES_DIR/expected-merged.toml" "$HOME/.codex/config.toml"
}

@test "self config-merge without BASE errors exactly" {
  run bash "$CODEX_SESSION_BIN" self config-merge
  [ "$status" -eq 1 ]
  [ "$output" = "ERROR: base config not found at $HOME/.codex/config.base.toml" ]
}

@test "symlinked entrypoint still resolves project root" {
  mkdir -p "$BATS_TEST_TMPDIR/linkbin"
  ln -s "$CODEX_SESSION_BIN" "$BATS_TEST_TMPDIR/linkbin/codex-session"
  run bash "$BATS_TEST_TMPDIR/linkbin/codex-session" self version
  [ "$status" -eq 0 ]
  [ "$output" = "codex-session 0.1.0" ]
}

@test "unknown self verb exits 2 and prints guidance" {
  run bash "$CODEX_SESSION_BIN" self nope
  [ "$status" -eq 2 ]
  [[ "$output" == *"unknown self verb: nope"* ]]
  [[ "$output" == *"run: codex-session self help"* ]]
}

# --- Parity checklist (Step 21) structural assertions -------------------------
# Items 2, 15, and 17 are structural invariants of the source that cannot be
# observed purely via runtime behavior. Lock them with grep assertions so a
# refactor that breaks them fails the suite.

@test "parity item 2: codex resolution uses command -v only" {
  # Use POSIX character classes (`[[:space:]]`) instead of GNU `\b`/`\s` so
  # the suite runs unchanged on BSD/macOS grep.
  run grep -E '(^|[[:space:]])command -v codex([[:space:]]|$)' "$TEST_ROOT/lib/passthrough.bash"
  [ "$status" -eq 0 ]
  # Disallow `which codex` or hard-coded absolute paths to a codex binary.
  run grep -E '(^|[[:space:]])which[[:space:]]+codex([[:space:]]|$)' "$TEST_ROOT/lib/passthrough.bash"
  [ "$status" -ne 0 ]
  run grep -E '"/(usr/(local/)?)?bin/codex"' "$TEST_ROOT/lib/passthrough.bash"
  [ "$status" -ne 0 ]
}

@test "parity item 15: merge uses TARGET.tmp.\$\$ and mv -f" {
  # shellcheck disable=SC2016  # grep patterns are literal; single quotes are intentional
  run grep -F '${TARGET}.tmp.$$' "$TEST_ROOT/lib/config_merge.bash"
  [ "$status" -eq 0 ]
  # shellcheck disable=SC2016
  run grep -E '^[[:space:]]*mv -f "\$tmp" "\$TARGET"[[:space:]]*$' "$TEST_ROOT/lib/config_merge.bash"
  [ "$status" -eq 0 ]
  # Disallow non-atomic alternatives.
  # shellcheck disable=SC2016
  run grep -E '(^|[[:space:]])cp[[:space:]]+"\$tmp"' "$TEST_ROOT/lib/config_merge.bash"
  [ "$status" -ne 0 ]
  # shellcheck disable=SC2016
  run grep -E '(^|[[:space:]])install[[:space:]]+.*"\$tmp"' "$TEST_ROOT/lib/config_merge.bash"
  [ "$status" -ne 0 ]
}

@test "parity item 17: nothing executes between merge and final exec" {
  # Last non-blank/non-comment lines of __exec_real_codex must be the
  # conditional merge and an exec, with no statements in between. Use POSIX
  # character classes for portability across GNU and BSD grep.
  run bash -c '
    awk "/^__exec_real_codex\\(\\)/{f=1} f{print} f && /^}/{exit}" \
      "$1" \
      | grep -vE "^[[:space:]]*(#|$)" \
      | tail -4
  ' _ "$TEST_ROOT/lib/passthrough.bash"
  [ "$status" -eq 0 ]
  # Expect, in order: `__merge_config`, `fi`, `exec "$real_codex" "$@"`, `}`
  printf '%s\n' "$output"
  [[ "$output" == *"__merge_config"* ]]
  last_exec="$(printf '%s\n' "$output" | grep -E '^[[:space:]]*exec ' | tail -1 | sed -E 's/^[[:space:]]+//')"
  # shellcheck disable=SC2016  # literal source-code string to assert against
  [ "$last_exec" = 'exec "$real_codex" "$@"' ]
  # No statements after exec (the only remaining line should be the closing `}`).
  tail_line="$(printf '%s\n' "$output" | tail -1 | sed 's/[[:space:]]//g')"
  [ "$tail_line" = "}" ]
}

@test "self config-status reports needs_merge: no when BASE is missing" {
  # Even without BASE on disk, the status command must mirror the pass-through
  # guard and report no, because no merge can actually run without BASE.
  run bash "$CODEX_SESSION_BIN" self config-status
  [ "$status" -eq 0 ]
  [[ "$output" == *"base:        $HOME/.codex/config.base.toml (exists=false)"* ]]
  [[ "$output" == *"needs_merge: no"* ]]
}
