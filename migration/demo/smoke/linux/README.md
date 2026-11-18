# Linux x86-64 desktop smoke procedure

Run on an x86-64 Linux machine with a real X11 or Wayland desktop and the D29
staged package. Record whether `XDG_SESSION_TYPE`, `WAYLAND_DISPLAY`, or
`DISPLAY` selected the active backend. A successful compile/stage does not
prove the Slint window opened or rendered.

1. Record distro/kernel, `uname -m`, Slint backend, binary SHA-256, desktop
   PID, owned server child PID (if observable), and assigned loopback origin.
2. Run `run.sh /path/with spaces/photo-culler-desktop` in the graphical
   session. Confirm the library window opens and visibly shows the
   `demo-001.jpg` thumbnail; save a screenshot.
3. From another terminal, run `probe.sh http://127.0.0.1:PORT` and save its
   output. It checks API readiness and the thumbnail content type only.
4. Close normally. Record that the owned child exited and its port was
   released. Reopen, repeat the API checks, and capture another screenshot.
5. Connect to a separate external service, close the app, then verify its PID
   and health endpoint remain live. Stop it manually after verification.

Complete `evidence.txt`. If the real Slint package or external connection
control is missing, mark the step blocked and name the integration dependency.

## Backend prerequisites

Build-time: C/C++ toolchain and the distro's Slint/winit X11 and Wayland
development headers (including Xlib/XKB, xcb, xrandr, xkbcommon and Wayland).
Runtime: an active X11 or Wayland display session and its compositor/display
server. The smoke runner records the selected session type; it does not create
a graphical session.
