#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  printf 'usage: %s <staging-directory>\n' "$0" >&2
  exit 2
fi
ROOT="$(cd -- "$1" && pwd -P)"
if find "$ROOT" -type l -print -quit | grep -q .; then
  printf 'symlinks are not allowed inside the web bundle\n' >&2
  exit 2
fi
MANIFEST="$ROOT/bundle.json"
if [[ ! -f "$MANIFEST" || -L "$MANIFEST" ]]; then
  printf 'bundle.json is missing or not a regular file\n' >&2
  exit 2
fi
if ! jq -e '.schema_version == 1 and (.files | type == "array" and length > 0) and ([.files[] | select(type != "string")] | length == 0)' "$MANIFEST" >/dev/null; then
  printf 'bundle.json must use schema_version 1 with a non-empty string files array\n' >&2
  exit 2
fi

required=(index.html bootstrap.js photo_culler_slint.js photo_culler_slint_bg.wasm)
for name in "${required[@]}"; do
  if ! jq -e --arg name "$name" '.files | index($name) != null' "$MANIFEST" >/dev/null; then
    printf 'required asset is outside bundle.json file allowlist: %s\n' "$name" >&2
    exit 2
  fi
done

while IFS= read -r relative; do
  if [[ -z "$relative" || "$relative" == /* || "$relative" == *'\\'* || ! "$relative" =~ ^[A-Za-z0-9._/-]+$ ]]; then
    printf 'unsafe manifest path: %s\n' "$relative" >&2
    exit 2
  fi
  IFS='/' read -r -a segments <<< "$relative"
  for segment in "${segments[@]}"; do
    if [[ -z "$segment" || "$segment" == . || "$segment" == .. ]]; then
      printf 'unsafe manifest path: %s\n' "$relative" >&2
      exit 2
    fi
  done
  path="$ROOT/$relative"
  if [[ -L "$path" || ! -f "$path" ]]; then
    printf 'declared asset is missing or not a regular file: %s\n' "$relative" >&2
    exit 2
  fi
done < <(jq -r '.files[]' "$MANIFEST")

if [[ "$(jq -r '.files | unique | length' "$MANIFEST")" != "$(jq -r '.files | length' "$MANIFEST")" ]]; then
  printf 'bundle.json has duplicate asset paths\n' >&2
  exit 2
fi
while IFS= read -r actual; do
  relative="${actual#"$ROOT/"}"
  [[ "$relative" == bundle.json ]] && continue
  if ! jq -e --arg name "$relative" '.files | index($name) != null' "$MANIFEST" >/dev/null; then
    printf 'staged asset is not declared in bundle.json: %s\n' "$relative" >&2
    exit 2
  fi
done < <(find "$ROOT" -type f -print)

printf 'Bundle valid: %s declared files; manifest is not served as an asset.\n' "$(jq -r '.files | length' "$MANIFEST")"
