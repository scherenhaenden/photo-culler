#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
python_bin="${PHOTO_CULLER_PYTHON:-${project_root}/.venv/bin/python}"
build_dir="${project_root}/builds/linux/egui"
work_dir="${project_root}/builds/.pyinstaller/egui"

cd "${project_root}/rust"
cargo build --release -p photo-culler-egui

cd "${project_root}"
mkdir -p "${build_dir}" "${work_dir}"
"${python_bin}" -m PyInstaller --noconfirm --clean --onefile --windowed \
  --name photo-culler-egui --workpath "${work_dir}/work" --specpath "${work_dir}" --distpath "${build_dir}" \
  --add-binary "${project_root}/rust/target/release/photo-culler-egui:." \
  --add-data "${project_root}/photo_culler/web/static:photo_culler/web/static" \
  --add-data "${project_root}/photo_culler/web/templates:photo_culler/web/templates" \
  photo_culler/desktop/egui_launcher.py
chmod +x "${build_dir}/photo-culler-egui"
cp packaging/linux/README.txt "${build_dir}/README.txt"
echo "Native egui Linux package created at ${build_dir}/photo-culler-egui"
