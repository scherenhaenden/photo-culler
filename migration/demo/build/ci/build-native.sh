#!/usr/bin/env bash
set -euo pipefail

usage() { printf 'usage: build-native.sh TARGET OUTPUT_DIR WORKSPACE_ROOT\n' >&2; exit 2; }
[[ $# -eq 3 ]] || usage
target="$1"
output_dir="$2"
root="$(cd -- "$3" && pwd -P)"

case "$target" in
  aarch64-apple-darwin) stage_script="migration/demo/build/macos/build.sh" ;;
  x86_64-unknown-linux-gnu) stage_script="migration/demo/build/linux/build.sh" ;;
  *) printf 'unsupported native CI target: %s\n' "$target" >&2; exit 2 ;;
esac

python3 "$root/migration/demo/build/ci/verify-packages.py" "$root"
for package in photo-culler-api photo-culler-client photo-culler-server photo-culler-slint; do
  cargo build --locked --manifest-path "$root/rust/Cargo.toml" --release --target "$target" --package "$package"
done

release_dir="$root/rust/target/$target/release"
desktop_bin="$release_dir/photo-culler-desktop"
server_bin="$release_dir/photo-culler-server"
[[ -x "$desktop_bin" ]] || { printf 'required package output is missing: %s\n' "$desktop_bin" >&2; exit 2; }
[[ -x "$server_bin" ]] || { printf 'required package output is missing: %s\n' "$server_bin" >&2; exit 2; }
"$root/$stage_script" --stage "$output_dir" "$desktop_bin" "$server_bin"
python3 "$root/migration/demo/build/ci/write-provenance.py" "$root" "$output_dir" "$target" compile-and-stage photo-culler-api,photo-culler-client,photo-culler-server,photo-culler-slint
