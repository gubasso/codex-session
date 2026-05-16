# shellcheck shell=bash

__cmd_self() {
  local verb="${1:-help}"
  shift || true
  case "$verb" in
    help)
      # shellcheck source=/dev/null
      source "$CODEX_SESSION_ROOT/commands/self_help.bash"
      __cmd_self_help "$@"
      ;;
    version)
      # shellcheck source=/dev/null
      source "$CODEX_SESSION_ROOT/commands/self_version.bash"
      __cmd_self_version "$@"
      ;;
    config-status)
      # shellcheck source=/dev/null
      source "$CODEX_SESSION_ROOT/commands/self_config_status.bash"
      __cmd_self_config_status "$@"
      ;;
    config-merge)
      # shellcheck source=/dev/null
      source "$CODEX_SESSION_ROOT/commands/self_config_merge.bash"
      __cmd_self_config_merge "$@"
      ;;
    show-local)
      # shellcheck source=/dev/null
      source "$CODEX_SESSION_ROOT/commands/self_show_local.bash"
      __cmd_self_show_local "$@"
      ;;
    *)
      __log_err "unknown self verb: $verb"
      __log_err "run: codex-session self help"
      return 2
      ;;
  esac
}
