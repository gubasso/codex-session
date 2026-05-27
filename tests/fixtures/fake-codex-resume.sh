#!/usr/bin/env bash
set -euo pipefail

case "${1-}" in
  exec)
    if [[ "${2-}" == "resume" ]]; then
      thread_id="${3-}"
      rollout="$CODEX_HOME/sessions/2026/05/22/rollout-${thread_id}.jsonl"
      if [[ -f "$rollout" ]]; then
        printf 'resumed:%s\n' "$thread_id"
        exit 0
      fi
      printf 'no rollout found for %s\n' "$thread_id" >&2
      exit 2
    fi

    thread_id="r1-test-thread"
    rollout_dir="$CODEX_HOME/sessions/2026/05/22"
    mkdir -p "$rollout_dir"
    printf '{"type":"thread.started","thread_id":"%s"}\n' "$thread_id" \
      > "$rollout_dir/rollout-${thread_id}.jsonl"
    printf '{"type":"thread.started","thread_id":"%s"}\n' "$thread_id"
    ;;
  resume)
    thread_id="${2-}"
    rollout="$CODEX_HOME/sessions/2026/05/22/rollout-${thread_id}.jsonl"
    if [[ -f "$rollout" ]]; then
      printf 'resumed:%s\n' "$thread_id"
      exit 0
    fi
    printf 'no rollout found for %s\n' "$thread_id" >&2
    exit 2
    ;;
  *)
    printf 'fake-codex: unrecognized args: %s\n' "$*" >&2
    exit 64
    ;;
esac
