#!/bin/sh
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
tmp=$(mktemp -d "${TMPDIR:-/tmp}/photo-culler-d04.XXXXXXXX")
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
mkdir -p "$tmp/src/tests"
cp "$here/lifecycle.rs" "$tmp/src/"
cp "$here/tests/lifecycle.rs" "$tmp/src/tests/"
cat > "$tmp/Cargo.toml" <<'TOML'
[package]
name = "photo-culler-d04-lifecycle"
version = "0.1.0"
edition = "2024"
[dependencies]
axum = "=0.7.9"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["macros", "rt-multi-thread", "net", "sync", "io-util"] }
TOML
printf 'pub mod lifecycle;\n' > "$tmp/src/lib.rs"
cargo test --manifest-path "$tmp/Cargo.toml" "$@"
