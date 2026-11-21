#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/../../../.." && pwd)"
TARGET="wasm32-unknown-unknown"
MANIFEST_PATH="${SLINT_MANIFEST:-$REPO_ROOT/rust/apps/photo-culler-slint/Cargo.toml}"
OUT_DIR="${WEB_OUT_DIR:-$REPO_ROOT/migration/demo/build/web-dist}"
RUSTUP_CMD="${RUSTUP_BIN:-rustup}"
CARGO_CMD="${CARGO_BIN:-cargo}"
WASM_BINDGEN_CMD="${WASM_BINDGEN_BIN:-wasm-bindgen}"
LOCKFILE="$REPO_ROOT/rust/Cargo.lock"

fail() {
  printf 'wasm-build: error: %s\n' "$1" >&2
  exit 2
}

if [[ ! -f "$LOCKFILE" ]]; then
  fail "Cargo.lock not found at $LOCKFILE (run from a complete Photo Culler checkout)"
fi

if ! command -v "$RUSTUP_CMD" >/dev/null 2>&1; then
  fail "rustup was not found; install the Rust toolchain before building WASM"
fi
if ! "$RUSTUP_CMD" target list --installed | grep -Fxq "$TARGET"; then
  fail "Rust target $TARGET is missing; install it with: rustup target add $TARGET"
fi

if ! command -v "$WASM_BINDGEN_CMD" >/dev/null 2>&1; then
  fail "wasm-bindgen CLI was not found; install the version pinned by Cargo.lock"
fi
LOCKED_BINDGEN_VERSION="$(awk '
  /^name = "wasm-bindgen"$/ { in_package = 1; next }
  in_package && /^version = / { gsub(/"/, "", $3); print $3; exit }
  in_package && /^\[/ { in_package = 0 }
' "$LOCKFILE")"
if [[ -z "$LOCKED_BINDGEN_VERSION" ]]; then
  fail "Cargo.lock has no wasm-bindgen package entry; resolve the Slint WASM dependencies first"
fi
CLI_VERSION_OUTPUT="$("$WASM_BINDGEN_CMD" --version 2>&1)" || fail "could not query wasm-bindgen CLI version: $CLI_VERSION_OUTPUT"
CLI_VERSION="${CLI_VERSION_OUTPUT##* }"
if [[ "$CLI_VERSION" != "$LOCKED_BINDGEN_VERSION" ]]; then
  fail "wasm-bindgen CLI $CLI_VERSION does not match Cargo.lock $LOCKED_BINDGEN_VERSION; install wasm-bindgen-cli $LOCKED_BINDGEN_VERSION"
fi

if [[ ! -f "$MANIFEST_PATH" ]]; then
  fail "Slint scaffold manifest is missing at $MANIFEST_PATH; integrate D11 before running the WASM build"
fi

mkdir -p "$OUT_DIR"
"$CARGO_CMD" build --locked --manifest-path "$MANIFEST_PATH" --target "$TARGET" --release
WASM_FILE="${WASM_FILE:-$REPO_ROOT/rust/target/$TARGET/release/photo_culler_slint.wasm}"
if [[ ! -f "$WASM_FILE" ]]; then
  fail "WASM build succeeded but expected input is missing: $WASM_FILE"
fi
"$WASM_BINDGEN_CMD" "$WASM_FILE" --target web --out-dir "$OUT_DIR"
printf 'WASM build complete. D23 input: %s\n' "$OUT_DIR/photo_culler_slint.js"
