#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
TEMP="$(mktemp -d)"
trap 'rm -rf "$TEMP"' EXIT
SOURCE="$TEMP/generated"
DEST="$TEMP/staged"
mkdir -p "$SOURCE/assets"
printf '<canvas id="canvas"></canvas>\n' > "$SOURCE/index.html"
printf 'bootstrap fixture\n' > "$SOURCE/bootstrap.js"
printf 'generated adapter fixture\n' > "$SOURCE/photo_culler_slint.js"
printf '\0asm\1\0\0\0' > "$SOURCE/photo_culler_slint_bg.wasm"
printf '<svg></svg>\n' > "$SOURCE/assets/icon.svg"

"$SCRIPT_DIR/stage.sh" "$SOURCE" "$DEST"
[[ "$(jq -r '.schema_version' "$DEST/bundle.json")" == 1 ]]
"$SCRIPT_DIR/validate.sh" "$DEST"

cp -R "$DEST" "$TEMP/missing"
rm "$TEMP/missing/photo_culler_slint.js"
if "$SCRIPT_DIR/validate.sh" "$TEMP/missing" >"$TEMP/missing.out" 2>&1; then
  printf 'missing declared asset unexpectedly passed validation\n' >&2; exit 1
fi

cp -R "$DEST" "$TEMP/unsafe"
jq '.files += ["../outside.wasm"]' "$TEMP/unsafe/bundle.json" > "$TEMP/bundle.json"
mv "$TEMP/bundle.json" "$TEMP/unsafe/bundle.json"
if "$SCRIPT_DIR/validate.sh" "$TEMP/unsafe" >"$TEMP/unsafe.out" 2>&1; then
  printf 'out-of-root manifest entry unexpectedly passed validation\n' >&2; exit 1
fi

cp -R "$DEST" "$TEMP/extra"
printf 'not declared\n' > "$TEMP/extra/unlisted.js"
if "$SCRIPT_DIR/validate.sh" "$TEMP/extra" >"$TEMP/extra.out" 2>&1; then
  printf 'undeclared staged asset unexpectedly passed validation\n' >&2; exit 1
fi
printf 'D23 staging/manifest checks passed.\n'
