#!/usr/bin/env bash
set -euo pipefail

usage() { printf 'usage: build-wasm.sh OUTPUT_DIR WORKSPACE_ROOT\n' >&2; exit 2; }
[[ $# -eq 2 ]] || usage
output_dir="$1"
root="$(cd -- "$2" && pwd -P)"
target=wasm32-unknown-unknown

python3 "$root/migration/demo/build/ci/verify-packages.py" "$root"
if ! rustup target list --installed | grep -Fxq "$target"; then
  printf 'required Rust target is not installed: %s\n' "$target" >&2
  exit 2
fi
locked_bindgen="$(awk '/^name = "wasm-bindgen"$/ { in_package=1; next } in_package && /^version = / { gsub(/"/, "", $3); print $3; exit } in_package && /^\[/ { in_package=0 }' "$root/rust/Cargo.lock")"
[[ -n "$locked_bindgen" ]] || { printf 'wasm-bindgen is missing from rust/Cargo.lock\n' >&2; exit 2; }
cli_version="$(wasm-bindgen --version | awk '{print $2}')"
[[ "$cli_version" == "$locked_bindgen" ]] || {
  printf 'wasm-bindgen CLI %s does not match Cargo.lock %s\n' "$cli_version" "$locked_bindgen" >&2
  exit 2
}

for package in photo-culler-api photo-culler-client photo-culler-slint; do
  cargo build --locked --manifest-path "$root/rust/Cargo.toml" --release --target "$target" --package "$package" --lib
done

wasm_file="$root/rust/target/$target/release/photo_culler_slint.wasm"
[[ -f "$wasm_file" ]] || { printf 'required Slint WASM output is missing: %s\n' "$wasm_file" >&2; exit 2; }
generated="$(mktemp -d "${TMPDIR:-/tmp}/photo-culler-wasm-ci.XXXXXXXX")"
trap 'rm -rf "$generated"' EXIT
wasm-bindgen "$wasm_file" --target web --out-dir "$generated" --out-name photo_culler_slint
cp "$root/rust/apps/photo-culler-slint/src/browser/index.html" "$generated/index.html"
cp "$root/rust/apps/photo-culler-slint/src/browser/bootstrap.js" "$generated/bootstrap.js"
"$root/migration/demo/build/web-dist/stage.sh" "$generated" "$output_dir/bundle"
python3 "$root/migration/demo/build/ci/write-provenance.py" "$root" "$output_dir" "$target" compile-and-bundle photo-culler-api,photo-culler-client,photo-culler-slint
