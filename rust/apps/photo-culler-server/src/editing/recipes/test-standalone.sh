#!/bin/sh
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
tmp=$(mktemp -d "${TMPDIR:-/tmp}/photo-culler-recipes-test.XXXXXXXX")
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
mkdir -p "$tmp/src"
cp "$here/mod.rs" "$tmp/src/lib.rs"
cat > "$tmp/Cargo.toml" <<'TOML'
[package]
name = "photo-culler-recipes-test"
version = "0.1.0"
edition = "2024"
[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
TOML
cargo test --manifest-path "$tmp/Cargo.toml" "$@"
