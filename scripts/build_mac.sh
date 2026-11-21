#!/usr/bin/env bash
# macOS Build Script for Photo Culler
set -euo pipefail

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
python_bin="${project_root}/.venv/bin/python"

if [[ ! -x "${python_bin}" ]]; then
  if command -v python3 >/dev/null 2>&1; then
    python_bin="$(command -v python3)"
  else
    echo "Python 3 is required." >&2
    exit 1
  fi
fi

cd "${project_root}"

# 1. Desktop App Build (pywebview / macOS GUI)
desktop_out="${project_root}/builds/desktop/mac"
mkdir -p "${desktop_out}" builds/.pyinstaller

"${python_bin}" -m PyInstaller \
  --noconfirm \
  --clean \
  --onefile \
  --windowed \
  --name photo-culler \
  --workpath "builds/.pyinstaller/work_mac" \
  --specpath "builds/.pyinstaller" \
  --distpath "${desktop_out}" \
  --hidden-import rawpy \
  --add-data "${project_root}/photo_culler/web/static:photo_culler/web/static" \
  --add-data "${project_root}/photo_culler/web/templates:photo_culler/web/templates" \
  photo_culler/desktop/app.py

chmod +x "${desktop_out}/photo-culler"
echo "macOS Desktop build created at ${desktop_out}/photo-culler"

# 2. Web App Server Launcher Build (FastAPI + HTMX)
web_out="${project_root}/builds/web/mac"
mkdir -p "${web_out}"

cat <<'EOF' > "${web_out}/photo-culler-web"
#!/usr/bin/env bash
# Photo Culler Web Server Launcher for macOS
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${DIR}/../../.." && pwd)"
if [[ -x "${PROJECT_ROOT}/.venv/bin/photo-culler" ]]; then
  exec "${PROJECT_ROOT}/.venv/bin/photo-culler" web "$@"
else
  exec python3 -m photo_culler.cli.main web "$@"
fi
EOF

chmod +x "${web_out}/photo-culler-web"
echo "macOS Web Server launcher created at ${web_out}/photo-culler-web"
