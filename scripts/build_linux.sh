#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
python_bin="${PHOTO_CULLER_PYTHON:-${project_root}/.venv/bin/python}"
output_dir="${PHOTO_CULLER_LINUX_BUILD_DIR:-${project_root}/builds/linux}"

if [[ ! -x "${python_bin}" ]]; then
  # CI installs the project into the runner Python rather than a local .venv.
  # Keep the local virtual environment as the default, but make the script
  # usable in both contexts without duplicating the build command.
  if command -v python3 >/dev/null 2>&1; then
    python_bin="$(command -v python3)"
  elif command -v python >/dev/null 2>&1; then
    python_bin="$(command -v python)"
  else
    echo "No Python interpreter found. Create .venv and install '.[linux,build]'."
    exit 1
  fi
fi

cd "${project_root}"
mkdir -p builds/.pyinstaller "${output_dir}"
"${python_bin}" -m PyInstaller \
  --noconfirm \
  --clean \
  --onefile \
  --windowed \
  --name photo-culler \
  --workpath "builds/.pyinstaller/work" \
  --specpath "builds/.pyinstaller" \
  --distpath "${output_dir}" \
  --exclude-module PyQt5 \
  --exclude-module PyQt6 \
  --exclude-module gi \
  --exclude-module webview \
  --hidden-import rawpy \
  --add-data "${project_root}/photo_culler/web/static:photo_culler/web/static" \
  --add-data "${project_root}/photo_culler/web/templates:photo_culler/web/templates" \
  photo_culler/desktop/linux_launcher.py

chmod +x "${output_dir}/photo-culler"
cp packaging/linux/README.txt "${output_dir}/README.txt"
echo "Linux desktop build created at ${output_dir}/photo-culler"
