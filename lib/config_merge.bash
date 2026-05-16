# shellcheck shell=bash

__needs_merge() {
  [[ ! -f "$TARGET" ]] && return 0
  [[ ! -f "$STAMP" ]]  && return 0
  [[ "$BASE" -nt "$STAMP" ]]
}

__extract_local_sections() {
  [[ -f "$TARGET" ]] || return 0
  awk '
    NR == FNR { base_headers[$0] = 1; next }
    /^\[/ { local = !($0 in base_headers) }
    local { print }
  ' <(grep -E '^\[' "$BASE") "$TARGET"
}

__merge_config() {
  local local_sections tmp
  local_sections="$(__extract_local_sections)"
  tmp="${TARGET}.tmp.$$"
  cat "$BASE" >"$tmp"
  if [[ -n "$local_sections" ]]; then
    printf '\n# === Machine-local (preserved by codex-session) ===\n' >>"$tmp"
    printf '%s\n' "$local_sections" >>"$tmp"
  fi
  mv -f "$tmp" "$TARGET"
  mkdir -p "$CACHE_DIR"
  touch "$STAMP"
}
