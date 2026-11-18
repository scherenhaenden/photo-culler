#!/bin/sh
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
tmp=$(mktemp -d "${TMPDIR:-/tmp}/photo-culler-render-test.XXXXXXXX")
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
mkdir -p "$tmp/src/render"
cp "$here/mod.rs" "$here/tests.rs" "$tmp/src/render/"
cat > "$tmp/Cargo.toml" <<'TOML'
[package]
name = "photo-culler-render-test"
version = "0.1.0"
edition = "2024"
TOML
printf 'pub mod render;\n' > "$tmp/src/lib.rs"
cargo test --manifest-path "$tmp/Cargo.toml" "$@"
