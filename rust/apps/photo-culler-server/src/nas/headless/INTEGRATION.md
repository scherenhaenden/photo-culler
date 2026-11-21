# Task 55: headless NAS startup contract

This directory defines a dependency-free startup and authorization seam for a
NAS service. `mod.rs` is intentionally self-contained so its acceptance tests
run without adding a server crate to the shared Cargo workspace.

## Acceptance harness

From the repository root:

```sh
rustc --edition=2024 --test rust/apps/photo-culler-server/src/nas/headless/mod.rs -o /tmp/photo-culler-task-55-tests
/tmp/photo-culler-task-55-tests
```

The tests cover loopback by default, passing a persistent catalog location to
the startup adapter, read-only originals, scoped writable storage, thermal
validation, explicit LAN binding, and remote administrator authorization.

## Integration boundary

The historical task-55 baseline predates the current server package. In the
integration candidate, an Axum server and loopback-only lifecycle exist, but
`nas::headless` is still not registered in `lib.rs` or mounted in
`demo/runtime.rs`. It does not provide NAS HTTP status/configuration endpoints,
NAS-specific storage permissions, or shared session authentication. The task
55 module remains a standalone startup/policy contract, not an operational NAS
service.

The catalog callback in the harness is a double for the startup boundary, not
a persistence implementation. LAN token handling is likewise only an opaque
credential-presence check; secret loading, hashing, and session validation
belong to the shared authentication layer. The current Axum lifecycle rejects
non-loopback binds, and no NAS route is available to opt into LAN exposure.

The write-scope helper rejects lexical `.` and `..` traversal. The filesystem
adapter must still canonicalize configured roots and candidate paths and
protect against symlinks before opening any writable file.
