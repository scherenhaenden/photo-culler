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

The desktop executable accepts `--data-dir`, repeatable `--authorized-root`,
and optional `--gallery-id` arguments and forwards them to its owned sibling
server. Roots must be available at startup. There is not yet a native folder
picker; the operator supplies roots at process launch. The loopback server
continues to own its child process and the desktop and browser use the shared
REST client. S01 evidence does not establish acceptance of the v2 import UI.

The standalone Task 44 checks use local doubles and can be run without Cargo
wiring. They do not exercise the packaged launcher or import v2 at runtime:

```sh
rustc --edition=2024 --test \
  rust/apps/photo-culler-slint/src/desktop/mod.rs \
  -o /tmp/photo-culler-task44-tests
/tmp/photo-culler-task44-tests
```
