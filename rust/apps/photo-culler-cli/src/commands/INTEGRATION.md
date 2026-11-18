# Task 56: Rust CLI command contracts

`mod.rs` contains the command parser, REST request mapping, stable process
error codes, offline diagnostics, and legacy preselection CSV formatter. The
test double exercises the same `RestClient` seam expected from the future
HTTP adapter.

Run the standalone unit harness from the repository root:

```sh
rustc --edition=2024 --test rust/apps/photo-culler-cli/src/commands/mod.rs -o /tmp/photo-culler-task-56-tests
/tmp/photo-culler-task-56-tests
```

The CSV header and selected values match the current Python command:
`jpeg_path,raw_path,group,selected,representative_jpeg,quality,similarity`,
with `yes`/`no` selection values and four-decimal score strings supplied by
the selection result.

## Integration boundary

The Rust CLI manifest currently depends on the core and image-engine crates,
but has no HTTP client dependency or shared API crate. Its `main.rs` only
registers `status`, `backends`, `frontends`, and local `analyze`; it does not
declare or dispatch through `commands`. The server package/router is absent
from the workspace. Because this task's exclusive area is only
`apps/photo-culler-cli/src/commands/`, this delivery does not modify `main.rs`,
`Cargo.toml`, or `Cargo.lock`, and cannot honestly claim the subcommands are
available from the compiled binary or validated against a live/real fixture
server. Routes are proposed under `/api/v1` and must be reconciled with the
frozen API contract when the integrator adds the shared client and server.

Implemented commands in the seam: `init`, `doctor`/`diagnostics`, `status`,
`config`, `volumes`, `scan`, `analyze`, `group`, `bursts`, `sessions`, `photos`,
`decisions`, `preselect`, `verify`, `evaluate`, `report`, `nas`, `backends`, and
`frontends`. Diagnostics remain successful and informative when transport is
down. Usage, transport, and server-response failures map to exit codes 2, 3,
and 4 respectively; this stable Rust mapping still needs comparison with the
legacy CLI's exact failure-code conventions during integration.
