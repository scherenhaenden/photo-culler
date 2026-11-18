#!/bin/sh
set -eu
desktop=${1:?usage: run.sh /path/to/photo-culler-desktop}
[ -x "$desktop" ] || { echo "desktop executable is missing or not executable: $desktop" >&2; exit 2; }
arch=$(uname -m)
[ "$arch" = arm64 ] || { echo "expected Apple Silicon arm64, found $arch" >&2; exit 2; }
printf 'Launching macOS desktop: %s (arch=%s)\n' "$desktop" "$arch"
"$desktop" &
pid=$!
printf 'Desktop PID=%s; wait for the Slint window and record its assigned API origin.\n' "$pid"
wait "$pid"
