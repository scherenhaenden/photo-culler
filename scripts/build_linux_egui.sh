#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
python_bin="${PHOTO_CULLER_PYTHON:-${project_root}/.venv/bin/python}"
build_dir="${project_root}/builds/linux/egui"
work_dir="${project_root}/builds/.pyinstaller/egui"

if [[ ! -x "${python_bin}" ]]; then
  echo "Python interpreter is not executable: ${python_bin}" >&2
  exit 1
fi
if [[ ! -f "${project_root}/packaging/linux/README.txt" ]]; then
  echo "Linux package README is missing: ${project_root}/packaging/linux/README.txt" >&2
  exit 1
fi

cd "${project_root}/rust"
cargo build --release -p photo-culler-egui

cd "${project_root}"
mkdir -p "${build_dir}" "${work_dir}"
install -m 755 "${project_root}/rust/target/release/photo-culler-egui" "${work_dir}/photo-culler-egui-native"
"${python_bin}" -m PyInstaller --noconfirm --clean --onefile \
  --name photo-culler-egui --workpath "${work_dir}/work" --specpath "${work_dir}" --distpath "${build_dir}" \
  --add-binary "${work_dir}/photo-culler-egui-native:." \
  --add-data "${project_root}/photo_culler/web/static:photo_culler/web/static" \
  --add-data "${project_root}/photo_culler/web/templates:photo_culler/web/templates" \
  photo_culler/desktop/egui_launcher.py
chmod +x "${build_dir}/photo-culler-egui"
cp packaging/linux/README.txt "${build_dir}/README.txt"
echo "Native egui Linux package created at ${build_dir}/photo-culler-egui"
