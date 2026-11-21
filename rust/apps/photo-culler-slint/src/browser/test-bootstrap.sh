#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
BOOTSTRAP="$SCRIPT_DIR/bootstrap.js"
HTML="$SCRIPT_DIR/index.html"

grep -F 'import("./photo_culler_slint.js")' "$BOOTSTRAP" >/dev/null
grep -F 'await app.default()' "$BOOTSTRAP" >/dev/null
if grep -Eq 'fetch[[:space:]]*\(|api/v1' "$BOOTSTRAP"; then
  printf 'bootstrap must not issue application API requests\n' >&2
  exit 1
fi
grep -F '<canvas id="canvas"' "$HTML" >/dev/null
grep -F 'id="diagnostic" role="alert"' "$HTML" >/dev/null
if grep -F 'id="slint-canvas"' "$HTML" >/dev/null; then
  printf 'Slint canvas must use the documented id=canvas\n' >&2
  exit 1
fi
printf 'D22 bootstrap checks passed; JavaScript only loads WASM and displays startup diagnostics.\n'
