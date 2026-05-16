# shellcheck shell=bash
__cmd_self_config_status() {
  local needs="no"
  # Mirror the pass-through guard: when BASE is absent, no merge ever runs,
  # so report no even though __needs_merge (which assumes BASE exists) would
  # otherwise be true on a stamp-less machine.
  if [[ -f "$BASE" ]] && __needs_merge; then needs="yes"; fi
  printf 'base:        %s (exists=%s)\n' "$BASE"   "$([[ -f "$BASE"   ]] && echo true || echo false)"
  printf 'target:      %s (exists=%s)\n' "$TARGET" "$([[ -f "$TARGET" ]] && echo true || echo false)"
  printf 'stamp:       %s (exists=%s)\n' "$STAMP"  "$([[ -f "$STAMP"  ]] && echo true || echo false)"
  printf 'needs_merge: %s\n' "$needs"
}
