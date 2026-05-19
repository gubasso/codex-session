#!/usr/bin/env bash
set -euo pipefail
printf 'argc=%s\n' "$#"
i=0
for arg in "$@"; do
  printf 'argv[%s]=%q\n' "$i" "$arg"
  i=$((i + 1))
done
