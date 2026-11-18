#!/bin/sh
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
tmp=$(mktemp -d "${TMPDIR:-/tmp}/photo-culler-linux-stage.XXXXXXXX")
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
mkdir -p "$tmp/input files" "$tmp/output folder with spaces"
printf '%s\n' '#!/bin/sh' 'exit 0' > "$tmp/input files/photo-culler-desktop"
printf '%s\n' '#!/bin/sh' 'exit 0' > "$tmp/input files/photo-culler-server"
chmod 755 "$tmp/input files/photo-culler-desktop" "$tmp/input files/photo-culler-server"
"$here/build.sh" --stage "$tmp/output folder with spaces" "$tmp/input files/photo-culler-desktop" "$tmp/input files/photo-culler-server"
test -x "$tmp/output folder with spaces/photo-culler-desktop"
test -x "$tmp/output folder with spaces/photo-culler-server"
printf '%s\n' 'Linux staging acceptance passed'
