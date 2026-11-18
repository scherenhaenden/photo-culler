#!/bin/sh
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
tmp=$(mktemp -d "${TMPDIR:-/tmp}/photo-culler-task37-test.XXXXXXXX")
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
mkdir -p "$tmp/src/contact_sheets"
cp "$here/mod.rs" "$tmp/src/contact_sheets/"
cat > "$tmp/Cargo.toml" <<'TOML'
[package]
name = "photo-culler-contact-sheets-test"
version = "0.1.0"
edition = "2024"
[dependencies]
image = { version = "0.25", default-features = false, features = ["png"] }
TOML
printf 'pub mod contact_sheets;\n' > "$tmp/src/lib.rs"
cargo test --manifest-path "$tmp/Cargo.toml" "$@"
