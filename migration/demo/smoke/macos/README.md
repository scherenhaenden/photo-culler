# macOS arm64 desktop smoke procedure

Run this checklist with the real Slint package staged by D18 on an Apple
Silicon Mac. A successful build or the API probes below do not prove that the
window opened or rendered correctly.

## Setup and evidence

1. Record macOS version, `uname -m`, app/server SHA-256, desktop PID, server
   child PID if visible, and the assigned `http://127.0.0.1:<port>` origin.
2. Launch `photo-culler-desktop` from a copied staging directory whose path
   contains spaces. Capture a screenshot showing the library and the
   `demo-001.jpg` thumbnail.
3. Record the health and gallery probe output from `probe.sh` below.
4. Close the window normally. Record that the owned child exited and the port
   stopped accepting connections. Reopen the app and repeat health/gallery
   checks; capture a second screenshot.
5. Start a separate external demo server on another loopback port, launch the
   app with `PHOTO_CULLER_API_ORIGIN=http://127.0.0.1:<external-port>`, then
   close the window.
   Verify that the external server PID remains alive and that its health URL
   still responds. Stop that service manually after the check.

Fill in `evidence.txt` for each run. External-service selection is currently
an environment override, not an in-window option, and the assigned origin is
not displayed in the UI. Record both limits; do not substitute a build result
for a UI check.

## API probe

```sh
sh migration/demo/smoke/macos/probe.sh http://127.0.0.1:PORT
```

The probe checks HTTP 200, `status: ready`, the demo gallery contract, and a
JPEG thumbnail content type. It reports API evidence only, not a GUI result.
