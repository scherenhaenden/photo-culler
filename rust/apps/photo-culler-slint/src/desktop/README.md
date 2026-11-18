# Task 44: native Slint desktop lifecycle

This standalone module defines the desktop launcher, API connection and HTTP
transport boundaries. The sprint base does not yet contain the
`photo-culler-slint` crate, a Slint window, or a shared REST client, so platform
process spawning and the real HTTP adapter remain integration work.

The ownership rules are explicit:

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

When the Slint crate is added, its close callback should call `close`; the
launcher adapter should own the child process handle and its graceful shutdown,
and the native HTTP adapter should attach the credential using the shared REST
contract. The page and widget shell should use the same `/api/v1` route scheme
as the browser transport.

Acceptance tests use only local doubles and can be run without Cargo wiring:

```sh
rustc --edition=2024 --test \
  rust/apps/photo-culler-slint/src/desktop/mod.rs \
  -o /tmp/photo-culler-task44-tests
/tmp/photo-culler-task44-tests
```
