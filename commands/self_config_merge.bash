# shellcheck shell=bash
__cmd_self_config_merge() {
  [[ -f "$BASE" ]] || __die "ERROR: base config not found at $BASE"
  __merge_config
  printf 'merged: %s -> %s\n' "$BASE" "$TARGET"
}
