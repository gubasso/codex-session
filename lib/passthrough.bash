# shellcheck shell=bash

__resolve_real_codex() {
  command -v codex 2>/dev/null
}

__exec_real_codex() {
  local real_codex
  real_codex="$(__resolve_real_codex)" || __die_codex_missing
  [[ -n "$real_codex" ]] || __die_codex_missing

  if [[ ! -f "$BASE" ]]; then
    exec "$real_codex" "$@"
  fi

  if __needs_merge; then
    __merge_config
  fi

  exec "$real_codex" "$@"
}
