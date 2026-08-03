#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output_dir="${PHOTO_CULLER_TAURI_BUILD_DIR:-/home/edward/Development/photo-culler/builds/linux/tauri-webgl}"
python_bin="${PHOTO_CULLER_PYTHON:-${project_root}/.venv/bin/python}"
target_triple="$(rustc -vV | awk '/host:/{print $2}')"
sidecar_name="photo-culler-backend-${target_triple}"

if [[ ! -x "${python_bin}" ]]; then
  python_bin="$(command -v python3)"
fi

cd "${project_root}"
rm -rf "${output_dir}"
mkdir -p "${output_dir}" "rust/apps/photo-culler-tauri/binaries"
placeholder_path="rust/apps/photo-culler-tauri/binaries/${sidecar_name}"
placeholder_copy="$(mktemp)"
cp "${placeholder_path}" "${placeholder_copy}"
trap 'cp "${placeholder_copy}" "${placeholder_path}"; rm -f "${placeholder_copy}"' EXIT

"${python_bin}" -m PyInstaller \
  --noconfirm --clean --onefile --name "${sidecar_name}" \
  --workpath "${output_dir}/.pyinstaller/work" \
  --specpath "${output_dir}/.pyinstaller" \
  --distpath "${output_dir}/.sidecar" \
  --add-data "${project_root}/photo_culler/web/static:photo_culler/web/static" \
  --add-data "${project_root}/photo_culler/web/templates:photo_culler/web/templates" \
  photo_culler/desktop/tauri_backend.py

install -m 755 "${output_dir}/.sidecar/${sidecar_name}" "${placeholder_path}"
(cd rust/apps/photo-culler-tauri && npx --yes @tauri-apps/cli@2.11.0 build --bundles deb)

find rust/target/release/bundle -type f -name '*.deb' -exec cp {} "${output_dir}/" \;
test -n "$(find "${output_dir}" -maxdepth 1 -type f -name '*.deb' -print -quit)"
echo "Tauri/WebGL Linux DEB created in ${output_dir}"
