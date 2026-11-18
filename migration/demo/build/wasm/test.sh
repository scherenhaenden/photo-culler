#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/../../../.." && pwd)"
TEMP_ROOT="$(mktemp -d)"
trap 'rm -rf "$TEMP_ROOT"' EXIT
BIN="$TEMP_ROOT/bin"
mkdir -p "$BIN"

cat > "$BIN/rustup" <<'FAKE_RUSTUP'
#!/usr/bin/env bash
if [[ "$1 $2 $3" == "target list --installed" ]]; then
  printf '%s\n' "${FAKE_TARGETS:-}"
  exit 0
fi
exit 2
FAKE_RUSTUP
cat > "$BIN/wasm-bindgen" <<'FAKE_BINDGEN'
#!/usr/bin/env bash
if [[ "$1" == "--version" ]]; then
  printf 'wasm-bindgen %s\n' "${FAKE_BINDGEN_VERSION:-0.2.100}"
  exit 0
fi
if [[ -n "${FAKE_BINDGEN_LOG:-}" ]]; then printf '%s\n' "$*" >> "$FAKE_BINDGEN_LOG"; fi
exit 0
FAKE_BINDGEN
cat > "$BIN/cargo" <<'FAKE_CARGO'
#!/usr/bin/env bash
if [[ -n "${FAKE_CARGO_LOG:-}" ]]; then printf '%s\n' "$*" >> "$FAKE_CARGO_LOG"; fi
if [[ -n "${FAKE_WASM_FILE:-}" ]]; then mkdir -p "$(dirname "$FAKE_WASM_FILE")"; : > "$FAKE_WASM_FILE"; fi
exit 0
FAKE_CARGO
chmod +x "$BIN/rustup" "$BIN/wasm-bindgen" "$BIN/cargo"

FAKE_LOCK="$TEMP_ROOT/Cargo.lock"
awk '
  /^\[\[package\]\]$/ { print; next }
  /^name = "wasm-bindgen"$/ { print; print "version = \"0.2.100\""; in_package = 1; next }
  in_package && /^version = / { next }
  in_package && /^\[/ { in_package = 0 }
  { print }
' "$REPO_ROOT/rust/Cargo.lock" > "$FAKE_LOCK"
FAKE_MANIFEST="$TEMP_ROOT/Cargo.toml"
: > "$FAKE_MANIFEST"

expect_failure() {
  local expected="$1"
  shift
  local output="$TEMP_ROOT/output.txt"
  if env PATH="$BIN:/usr/bin:/bin" RUSTUP_BIN=rustup CARGO_BIN=cargo WASM_BINDGEN_BIN=wasm-bindgen \
      SLINT_MANIFEST="$FAKE_MANIFEST" LOCKFILE_OVERRIDE="$FAKE_LOCK" "$@" \
      "$SCRIPT_DIR/build.sh" >"$output" 2>&1; then
    printf 'expected failure containing: %s\n' "$expected" >&2
    exit 1
  fi
  grep -F "$expected" "$output" >/dev/null || { cat "$output" >&2; exit 1; }
}

# The script uses the repository lockfile, so isolate it with a temporary copy
# via a temporary checkout-shaped wrapper for each preflight case.
make_fixture() {
  local root="$TEMP_ROOT/fixture"
  mkdir -p "$root/migration/demo/build/wasm" "$root/rust"
  cp "$SCRIPT_DIR/build.sh" "$root/migration/demo/build/wasm/build.sh"
  cp "$FAKE_LOCK" "$root/rust/Cargo.lock"
  printf '%s\n' "$root"
}

MISSING_TARGET_ROOT="$(make_fixture)"
if PATH="$BIN:/usr/bin:/bin" RUSTUP_BIN=rustup CARGO_BIN=cargo WASM_BINDGEN_BIN=wasm-bindgen \
   FAKE_TARGETS="x86_64-apple-darwin" SLINT_MANIFEST="$FAKE_MANIFEST" \
   bash "$MISSING_TARGET_ROOT/migration/demo/build/wasm/build.sh" >"$TEMP_ROOT/missing.txt" 2>&1; then
  printf 'expected missing target to fail\n' >&2; exit 1
fi
grep -F 'install it with: rustup target add wasm32-unknown-unknown' "$TEMP_ROOT/missing.txt" >/dev/null

VERSION_ROOT="$(make_fixture)"
if PATH="$BIN:/usr/bin:/bin" RUSTUP_BIN=rustup CARGO_BIN=cargo WASM_BINDGEN_BIN=wasm-bindgen \
   FAKE_TARGETS="wasm32-unknown-unknown" FAKE_BINDGEN_VERSION=0.2.101 \
   SLINT_MANIFEST="$FAKE_MANIFEST" bash "$VERSION_ROOT/migration/demo/build/wasm/build.sh" >"$TEMP_ROOT/version.txt" 2>&1; then
  printf 'expected incompatible wasm-bindgen version to fail\n' >&2; exit 1
fi
grep -F 'does not match Cargo.lock 0.2.100' "$TEMP_ROOT/version.txt" >/dev/null

printf 'D20 preflight tests passed (missing target and CLI version mismatch). No WASM compilation was run.\n'
