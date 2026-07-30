#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
python_bin="${project_root}/.venv/bin/python"

if [[ ! -x "${python_bin}" ]]; then
  echo "Missing .venv. Create it and install the Linux build extras first:"
  echo "  uv venv --python 3.14"
  echo "  uv pip install -e '.[linux,build]'"
  exit 1
fi

cd "${project_root}"
"${python_bin}" -m PyInstaller \
  --noconfirm \
  --clean \
  --onefile \
  --windowed \
  --name photo-culler \
  --distpath "builds/linux" \
  --exclude-module PyQt5 \
  --exclude-module PyQt6 \
  --exclude-module gi \
  --exclude-module webview \
  --add-data "photo_culler/web/static:photo_culler/web/static" \
  --add-data "photo_culler/web/templates:photo_culler/web/templates" \
  photo_culler/desktop/linux_launcher.py

chmod +x builds/linux/photo-culler
cp packaging/linux/README.txt builds/linux/README.txt
echo "Linux desktop build created at ${project_root}/builds/linux/photo-culler"
