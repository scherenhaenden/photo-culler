#!/bin/sh
set -eu
desktop=${1:?usage: run.sh /path/to/photo-culler-desktop}
[ -x "$desktop" ] || { echo "desktop executable is missing or not executable: $desktop" >&2; exit 2; }
arch=$(uname -m)
[ "$arch" = x86_64 ] || { echo "expected x86_64, found $arch" >&2; exit 2; }
if [ -n "${WAYLAND_DISPLAY:-}" ]; then backend=wayland
elif [ -n "${DISPLAY:-}" ]; then backend=x11
else echo "no DISPLAY or WAYLAND_DISPLAY; run inside a graphical session" >&2; exit 2
fi
printf 'Launching %s (arch=%s backend=%s session=%s)\n' "$desktop" "$arch" "$backend" "${XDG_SESSION_TYPE:-unknown}"
exec "$desktop"
