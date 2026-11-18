#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  printf 'usage: %s <generated-web-output> <staging-directory>\n' "$0" >&2
  exit 2
fi
SOURCE="$(cd -- "$1" && pwd -P)"
DEST="$2"
mkdir -p "$DEST"
DEST="$(cd "$DEST" && pwd -P)"
if [[ "$SOURCE" == "$DEST" || "$SOURCE" == "$DEST"/* ]]; then
  printf 'source and staging directory must be separate\n' >&2
  exit 2
fi
if [[ -n "$(find "$DEST" -mindepth 1 -print -quit)" ]]; then
  printf 'staging directory must be empty: %s\n' "$DEST" >&2
  exit 2
fi
REQUIRED=(index.html bootstrap.js photo_culler_slint.js photo_culler_slint_bg.wasm)
FILES=()
for name in "${REQUIRED[@]}"; do
  source_file="$SOURCE/$name"
  if [[ ! -f "$source_file" || -L "$source_file" ]]; then
    printf 'missing required regular asset: %s\n' "$name" >&2
    exit 2
  fi
  cp "$source_file" "$DEST/$name"
  FILES+=("$name")
done

if [[ -d "$SOURCE/assets" ]]; then
  while IFS= read -r -d '' source_file; do
    relative="${source_file#"$SOURCE/"}"
    if [[ -L "$source_file" || "$relative" == *'..'* || "$relative" == *'\\'* || ! "$relative" =~ ^[A-Za-z0-9._/-]+$ ]]; then
      printf 'unsafe generated asset path: %s\n' "$relative" >&2
      exit 2
    fi
    mkdir -p "$DEST/$(dirname "$relative")"
    cp "$source_file" "$DEST/$relative"
    FILES+=("$relative")
  done < <(find "$SOURCE/assets" -type f -print0)
  if find "$SOURCE/assets" -type l -print -quit | grep -q .; then
    printf 'symlinked web assets are not allowed\n' >&2
    exit 2
  fi
fi

printf '%s\n' "${FILES[@]}" | jq -R . | jq -s '{schema_version: 1, files: .}' > "$DEST/bundle.json"
"$(dirname "$0")/validate.sh" "$DEST"
printf 'Staged same-origin web bundle at %s\n' "$DEST"
