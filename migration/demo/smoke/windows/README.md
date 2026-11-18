# Windows x86-64 desktop smoke procedure

Run on a real Windows desktop with the D31 package. Record Windows build,
architecture, Slint backend/runtime, SHA-256 of both binaries, desktop PID,
server child PID if visible, and assigned loopback origin. Use a staging path
that contains spaces.

1. Run `run.ps1 'C:\Program Files\Photo Culler Demo\photo-culler-desktop.exe'`.
   Confirm the Slint window opens and visibly renders `demo-001.jpg`; save a
   screenshot. Record the PID printed by the script.
2. Run `probe.ps1 http://127.0.0.1:PORT`; save the health/gallery/JPEG output.
3. Close normally. Verify and record that the owned server child exits and the
   port is released. Reopen and repeat the screenshot and API checks.
4. Start an independent external server, connect to it explicitly, close the
   desktop and verify the external PID and health endpoint remain alive. Stop
   that service manually after recording the result.
5. Complete `evidence.txt`. A CI build without an interactive desktop is only
   build evidence; leave graphical checks pending if no desktop is available.

The current launcher integration must expose the assigned origin and an
explicit external-service choice for all steps to be executable. Record any
missing UI capability as a blocker instead of inferring success from build or
API output.
