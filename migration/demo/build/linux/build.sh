#!/bin/sh
set -eu

usage() {
  cat <<'EOF'
Usage:
  build.sh --stage OUTPUT_DIR DESKTOP_BINARY SERVER_BINARY
  build.sh --build OUTPUT_DIR WORKSPACE_ROOT
EOF
}
fail() { printf '%s\n' "error: $*" >&2; exit 2; }
mode=${1:-}
case "$mode" in
  --stage)
    [ "$#" -eq 4 ] || { usage >&2; exit 2; }
    output_dir=$2 desktop_bin=$3 server_bin=$4
    ;;
  --build)
    [ "$#" -eq 3 ] || { usage >&2; exit 2; }
    output_dir=$2 workspace_root=$3
    command -v cargo >/dev/null 2>&1 || fail "cargo is required"
    cargo build --manifest-path "$workspace_root/rust/Cargo.toml" --release --target x86_64-unknown-linux-gnu -p photo-culler-slint -p photo-culler-server
    desktop_bin="$workspace_root/rust/target/x86_64-unknown-linux-gnu/release/photo-culler-desktop"
    server_bin="$workspace_root/rust/target/x86_64-unknown-linux-gnu/release/photo-culler-server"
    ;;
  *) usage >&2; exit 2 ;;
esac
[ -f "$desktop_bin" ] || fail "desktop executable missing: $desktop_bin"
[ -f "$server_bin" ] || fail "server executable missing: $server_bin"
mkdir -p "$output_dir"
install -m 755 "$desktop_bin" "$output_dir/photo-culler-desktop"
install -m 755 "$server_bin" "$output_dir/photo-culler-server"
chmod 755 "$output_dir/photo-culler-desktop" "$output_dir/photo-culler-server"
printf 'Staged Linux x86-64 demo in: %s\n' "$output_dir"
