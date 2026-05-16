# shellcheck shell=bash

__dispatch() {
  if [[ $# -ge 1 && "$1" == "self" ]]; then
    shift
    # shellcheck source=/dev/null
    source "$CODEX_SESSION_ROOT/commands/self.bash"
    __cmd_self "$@"
    return
  fi
  __exec_real_codex "$@"
}
