#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/../../.." && pwd)"
cd "$repo_root"

required_manifests=(
  rust/crates/photo-culler-api/Cargo.toml
  rust/crates/photo-culler-client/Cargo.toml
  rust/apps/photo-culler-server/Cargo.toml
  rust/apps/photo-culler-slint/Cargo.toml
)
missing=()
for manifest in "${required_manifests[@]}"; do
  if [[ ! -f "$manifest" ]]; then
    missing+=("$manifest")
  fi
done
if ((${#missing[@]})); then
  printf 'D33 pending: required package manifests are not integrated yet:\n' >&2
  printf '  - %s\n' "${missing[@]}" >&2
  exit 3
fi

metadata_file="$(mktemp)"
trap 'rm -f "$metadata_file"' EXIT
cargo metadata --locked --manifest-path rust/Cargo.toml >"$metadata_file"
python3 - "$metadata_file" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as metadata_file:
    metadata = json.load(metadata_file)
packages = {package["name"] for package in metadata["packages"]}
required = {
    "photo-culler-api",
    "photo-culler-client",
    "photo-culler-server",
    "photo-culler-slint",
}
missing = sorted(required - packages)
if missing:
    raise SystemExit("registered package(s) missing from cargo metadata: " + ", ".join(missing))
print("cargo metadata: all four D33 packages are registered")
PY

for package in photo-culler-api photo-culler-client photo-culler-server photo-culler-slint; do
  cargo check --locked --manifest-path rust/Cargo.toml -p "$package"
done

if rustup target list --installed | grep -qx 'wasm32-unknown-unknown'; then
  cargo check --locked --manifest-path rust/Cargo.toml -p photo-culler-api --target wasm32-unknown-unknown
else
  printf 'WASM check pending: install target wasm32-unknown-unknown\n'
fi
