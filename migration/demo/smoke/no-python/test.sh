#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
TEMP="$(mktemp -d)"
trap 'rm -rf "$TEMP"' EXIT
mkdir -p "$TEMP/bin"
cat > "$TEMP/bin/ps" <<'FAKE_PS'
#!/usr/bin/env bash
case "$2" in
  301) printf 'photo-culler-server --port 0\n' ;;
  302) printf 'python3 application.py\n' ;;
  *) exit 1 ;;
esac
FAKE_PS
cat > "$TEMP/bin/pgrep" <<'FAKE_PGREP'
#!/usr/bin/env bash
case "$2" in
  301) [[ "${FAKE_WITH_PYTHON:-0}" == 1 ]] && printf '302\n' ;;
  *) exit 1 ;;
esac
FAKE_PGREP
chmod +x "$TEMP/bin/ps" "$TEMP/bin/pgrep"

PS_BIN="$TEMP/bin/ps" PGREP_BIN="$TEMP/bin/pgrep" "$SCRIPT_DIR/verify-process-tree.sh" 301 > "$TEMP/clean.out"
if PS_BIN="$TEMP/bin/ps" PGREP_BIN="$TEMP/bin/pgrep" FAKE_WITH_PYTHON=1 "$SCRIPT_DIR/verify-process-tree.sh" 301 > "$TEMP/python.out" 2>&1; then
  printf 'Python child unexpectedly passed verification\n' >&2
  exit 1
fi
grep -F 'Python process found' "$TEMP/python.out" >/dev/null
printf 'D27 process-tree test passed (clean tree accepted; Python descendant rejected).\n'
