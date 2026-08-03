# Tauri + WebGL experiment

This is an experimental Linux Tauri shell around the production FastAPI UI. The
sidecar and package can be built, but Wayland navigation from the startup page to
the authenticated local UI is not reliable yet. Do not treat it as a promoted
desktop delivery.

At startup the shell launches its packaged Python sidecar on `127.0.0.1` with an
ephemeral port and cryptographically random session token. It waits for `/api/health`
before navigating the window and kills the sidecar when the Tauri process exits. The
service persists its catalog in the standard user-data location; it never exposes a
network listener or token outside the local desktop session.

Build packages with:

```bash
./scripts/build_tauri_linux.sh
```

The script writes the Linux `.deb` output to
`builds/linux/tauri-webgl/` below the repository root. Override that location
with `PHOTO_CULLER_TAURI_BUILD_DIR=/ruta/de/salida` when needed. Linux build
dependencies are `libwebkit2gtk-4.1-dev`, `libsoup-3.0-dev`, `libgtk-3-dev`,
`libayatana-appindicator3-dev` and `librsvg2-dev`; Node is used only to run the
pinned Tauri CLI. The source-sidecar placeholder deliberately exits with an error:
it is replaced temporarily by the PyInstaller backend only while packaging.
