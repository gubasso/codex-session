#!/usr/bin/env bash
set -euo pipefail
if [ "${1:-}" = "--version" ]; then
  exit 0
fi
for n in $(seq 1 100); do
  printf 'line-%d\n' "$n" >&2
  sleep 0.05
done
