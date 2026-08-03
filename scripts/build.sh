#!/usr/bin/env bash
# Build one or more locally supported Photo Culler artifacts.
set -euo pipefail

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output_dir="${project_root}/builds"
build_linux=0
build_rust_cli=0
build_rust_egui=0
build_rust_tauri=0
target_selection_explicit=0

usage() {
  cat <<'EOF'
Usage: ./scripts/build.sh [options]

Build targets:
  --linux             Python/Linux desktop executable (PyInstaller)
  --rust-cli          Rust command-line prototype
  --rust-egui         Rust egui prototype
  --rust-tauri        Rust Tauri prototype
  --all               Build every target above

Exclusions (use after --all):
  --no-linux          Skip the Python/Linux desktop executable
  --no-rust-cli       Skip the Rust CLI prototype
  --no-rust-egui      Skip the Rust egui prototype
  --no-rust-tauri     Skip the Rust Tauri prototype

Other options:
  --output DIR        Write final artifacts below DIR (default: ./builds)
  -h, --help          Show this help

With no target flag, --linux is selected. Rust targets are compile-safe
prototypes; only --linux creates the supported desktop application.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --linux) target_selection_explicit=1; build_linux=1 ;;
    --rust-cli) target_selection_explicit=1; build_rust_cli=1 ;;
    --rust-egui) target_selection_explicit=1; build_rust_egui=1 ;;
    --rust-tauri) target_selection_explicit=1; build_rust_tauri=1 ;;
    --all) target_selection_explicit=1; build_linux=1; build_rust_cli=1; build_rust_egui=1; build_rust_tauri=1 ;;
    --no-linux) target_selection_explicit=1; build_linux=0 ;;
    --no-rust-cli) target_selection_explicit=1; build_rust_cli=0 ;;
    --no-rust-egui) target_selection_explicit=1; build_rust_egui=0 ;;
    --no-rust-tauri) target_selection_explicit=1; build_rust_tauri=0 ;;
    --output)
      shift
      if [[ $# -eq 0 || "$1" == -* ]]; then
        echo "--output requires a directory." >&2
        exit 2
      fi
      output_dir="$1"
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
  shift
done

if [[ $target_selection_explicit -eq 0 ]]; then
  build_linux=1
elif [[ $build_linux -eq 0 && $build_rust_cli -eq 0 && $build_rust_egui -eq 0 && $build_rust_tauri -eq 0 ]]; then
  echo "No build targets selected." >&2
  exit 2
fi

mkdir -p "${output_dir}"

build_rust_target() {
  local package="$1"
  local binary="$2"
  local destination="${output_dir}/rust/${binary}"

  cargo build --manifest-path "${project_root}/rust/Cargo.toml" --release -p "${package}"
  mkdir -p "$(dirname "${destination}")"
  install -m 755 "${project_root}/rust/target/release/${binary}" "${destination}"
  echo "Rust build created at ${destination}"
}

if [[ $build_linux -eq 1 ]]; then
  PHOTO_CULLER_LINUX_BUILD_DIR="${output_dir}/linux" "${project_root}/scripts/build_linux.sh"
fi
if [[ $build_rust_cli -eq 1 ]]; then
  build_rust_target "photo-culler-cli" "photo-culler-cli"
fi
if [[ $build_rust_egui -eq 1 ]]; then
  build_rust_target "photo-culler-egui" "photo-culler-egui"
fi
if [[ $build_rust_tauri -eq 1 ]]; then
  build_rust_target "photo-culler-tauri" "photo-culler-tauri"
fi
