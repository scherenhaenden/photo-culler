#!/bin/sh
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
tmp=$(mktemp -d "${TMPDIR:-/tmp}/photo-culler-export-test.XXXXXXXX")
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
mkdir -p "$tmp/src/export"
cp "$here/mod.rs" "$tmp/src/export/"
cat > "$tmp/Cargo.toml" <<'TOML'
[package]
name = "photo-culler-export-test"
version = "0.1.0"
edition = "2021"
[lib]
path = "src/lib.rs"
TOML
printf 'pub mod export;\n' > "$tmp/src/lib.rs"
cargo test --manifest-path "$tmp/Cargo.toml" "$@"
