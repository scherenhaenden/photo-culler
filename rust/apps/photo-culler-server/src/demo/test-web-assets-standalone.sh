#!/bin/sh
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
tmp=$(mktemp -d "${TMPDIR:-/tmp}/photo-culler-d09.XXXXXXXX")
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
mkdir -p "$tmp/src"
cp "$here/web_assets.rs" "$tmp/src/lib.rs"
cat > "$tmp/Cargo.toml" <<'TOML'
[package]
name = "photo-culler-d09-web-assets"
version = "0.1.0"
edition = "2024"
[dependencies]
axum = "=0.7.9"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
tower = { version = "0.5", features = ["util"] }
TOML
cargo test --manifest-path "$tmp/Cargo.toml" "$@"
