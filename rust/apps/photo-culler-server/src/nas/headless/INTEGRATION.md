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

At the task 55 baseline, `rust/apps/photo-culler-server/Cargo.toml` and a server
crate root/router do not exist, and `apps/photo-culler-server` is not a member
of `rust/Cargo.toml`. Existing diagnostics, session, and origin modules are
standalone contracts rather than a running Axum service. This task therefore
does not claim an HTTP listener, REST status/configuration endpoints, SQLite
catalog persistence, or shared session integration. The integrator must add
the server package and adapt these contracts to the frozen API/session/router
interfaces before those runtime properties can be accepted.

The catalog callback in the harness is a double for the startup boundary, not
a persistence implementation. LAN token handling is likewise an opaque
credential-presence check; secret loading, hashing, and session validation
belong to the shared authentication layer.
