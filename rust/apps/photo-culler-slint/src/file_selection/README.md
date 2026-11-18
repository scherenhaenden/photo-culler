# File selection integration notes

This is a transport-independent platform boundary because the Slint app crate,
browser bridge, native dialog implementation, and shared API DTOs are not
present at the sprint base.

- The native host implements `NativePicker` and keeps `NativePath` in the
  desktop process. `None` maps to a stable cancellation result.
- The browser lists `AuthorizedRoot` values from the server and calls
  `browser_request` with a root ID plus a relative path. The `BrowserRequest`
  type has no local-path field; absolute paths, backslashes, dot segments,
  offline roots, unknown roots, and disallowed operations are rejected before
  encoding.
- Integrate by mapping `BrowserRequest` to the shared API DTO. The server must
  resolve the relative path under the authorized root and re-check policy;
  client checks are not a substitute for server authorization.
- The router must decide how directory selection is presented (for example,
  an authorized-root browser) and freeze error codes. This module's enum is
  an adapter contract, not the final HTTP shape.

Standalone acceptance:

```sh
rustc --edition=2021 --test rust/apps/photo-culler-slint/src/file_selection/mod.rs -o /tmp/photo-culler-task52-tests
/tmp/photo-culler-task52-tests
```
