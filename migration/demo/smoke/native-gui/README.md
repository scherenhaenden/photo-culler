# Native Slint GUI smoke

The GitHub Actions native matrix executes this smoke on macOS arm64, Linux
x86-64, and Windows x86-64 using the staged desktop and sibling server
binaries. Linux uses Xvfb plus Openbox; the other jobs use the hosted runner's
native window server. Each screenshot is cropped to the Slint top-level window.
No user desktop is involved. The Windows capture helper enables a per-monitor
DPI-aware thread before reading `GetWindowRect`, records the window and physical
primary-screen bounds from the display device context, and verifies that the
PNG dimensions match a window fully inside the physical display.

The runner-side Python script does not ship in the application or become a
descendant of the desktop. It copies both Rust executables into a temporary
directory containing spaces, launches the actual Slint binary, and waits for
its `PHOTO_CULLER_SMOKE_STATUS_FILE` event confirming that the REST client
decoded and published `demo-001.jpg` to the Library view. The screenshot loop
then retries for up to 20 seconds until it detects the fixture's yellow sun and
blue sky together inside the central upper gallery viewport, proving that
Slint painted the thumbnail. The pixel scan excludes the sidebar, filter
controls, and transparent window edges. It then:

1. Probes health, gallery JSON, and the real JPEG content type.
2. Captures the visible Slint window at the runner's native viewport.
3. Sends the platform window-close event and checks the app exits normally,
   the owned server child exits, and the assigned loopback port is released.
4. Reopens the app and repeats the owned-server lifecycle.
5. Starts a separate server and passes its origin with
   `PHOTO_CULLER_API_ORIGIN`; closing the app must leave that process and health
   endpoint running.

The Windows hosted runner lacks a dependable OpenGL 2.0 driver. Its GUI smoke
sets `SLINT_BACKEND=winit-software` to use Slint's software renderer with the
same Winit window and UI. FemtoVG remains the normal default renderer, and the
software renderer is also available as a fallback when no OpenGL driver exists.
The Windows evidence records the selected backend; native GUI acceptance does
not claim that FemtoVG/OpenGL was exercised on that runner.

Screenshots, API logs, process IDs, binary hashes, origin, and exit outcomes
are uploaded as `rust-slint-demo-native-gui-<platform>-<run-id>-<attempt>`.
A runner that cannot create or capture a visible Slint window fails the GUI
acceptance job; a build or API test alone is not reported as graphical proof.
