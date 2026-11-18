#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 || ! "$1" =~ ^[0-9]+$ ]]; then
  printf 'usage: %s <root-pid>\n' "$0" >&2
  exit 2
fi
ROOT_PID="$1"
PS_CMD="${PS_BIN:-ps}"
PGREP_CMD="${PGREP_BIN:-pgrep}"
SEEN="|"
FOUND_PYTHON=0

walk() {
  local pid="$1"
  [[ "$SEEN" == *"|$pid|"* ]] && return 0
  SEEN="${SEEN}${pid}|"
  local command
  command="$("$PS_CMD" -p "$pid" -o command= 2>/dev/null || true)"
  if [[ -z "$command" ]]; then
    printf 'process %s is not running or cannot be inspected\n' "$pid" >&2
    return 2
  fi
  printf '%s\t%s\n' "$pid" "$command"
  if printf '%s\n' "$command" | grep -Eiq '(^|/)(python([0-9.]*)?)( |$)|(^|[ /])python([0-9.]*)?([[:space:]]|$)'; then
    FOUND_PYTHON=1
  fi
  local child
  while IFS= read -r child; do
    [[ -n "$child" ]] && walk "$child"
  done < <("$PGREP_CMD" -P "$pid" 2>/dev/null || true)
}

walk "$ROOT_PID"
if [[ "$FOUND_PYTHON" -eq 1 ]]; then
  printf 'Python process found in package process tree\n' >&2
  exit 1
fi
printf 'No Python process found beneath PID %s\n' "$ROOT_PID"
