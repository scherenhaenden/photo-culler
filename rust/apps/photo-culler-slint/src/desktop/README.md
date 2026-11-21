# Native Slint desktop lifecycle

This package now contains the Slint desktop launcher, shared REST client and
native HTTP transport. The accepted S01 lifecycle evidence covers launching and
stopping the fixture server. The import v2 candidate adds local SQLite and
authorized-root arguments, but its end-to-end runtime behavior remains pending
acceptance; see `docs/rust-slint-demo/RUN.md` for commands and manual checks.

The ownership rules below document the older standalone `desktop/mod.rs`
contract. The current packaged entry point uses `desktop/owned_server.rs`; do
not treat these module-level checks as end-to-end import acceptance.

- `start_local` accepts only an HTTP loopback endpoint with a nonzero assigned
  port and a nonempty, unexpired session credential. It retains the launched
  process handle.
- `connect_external` keeps an explicitly supplied instance separate from an
  owned local process. The window never claims ownership of an external server.
- `recover_if_stopped` restarts only an owned process after failure and refreshes
  its endpoint and credential. An external server is not restarted by the
  desktop window.
- `close` and `Drop` stop only the owned process. They detach from an external
  connection without terminating it.
- `DesktopApiClient` sends requests under the selected origin, restricts paths
  to `/api/v1/`, and supplies the session credential as an HTTP
  `Authorization: Bearer …` header value, separate from the URL. Debug output
  redacts the credential.

With no arguments, the desktop opens its local profile and asks for a photo
folder on first use. Linux stores the profile and SQLite catalogue under
`$XDG_DATA_HOME/photo-culler/desktop` (or `~/.local/share/photo-culler/desktop`);
macOS uses `~/Library/Application Support/Photo Culler/Desktop`. The profile
stores only its schema version, authorized folder and known gallery ID. The
catalogue database remains server-owned. Folder selection uses the Linux
desktop portal and the native macOS folder dialog; selected paths stay in the
desktop process and are forwarded only to the loopback child server.

`--demo` explicitly launches the old fixture-only S01 experience for smoke
tests. `--data-dir`, repeatable `--authorized-root`, and optional `--gallery-id`
remain available for explicit local catalog runs. The desktop still owns its
loopback child process and uses the shared REST client.

The standalone Task 44 checks use local doubles and can be run without Cargo
wiring. They do not exercise the packaged launcher or import v2 at runtime:

```sh
rustc --edition=2024 --test \
  rust/apps/photo-culler-slint/src/desktop/mod.rs \
  -o /tmp/photo-culler-task44-tests
/tmp/photo-culler-task44-tests
```
