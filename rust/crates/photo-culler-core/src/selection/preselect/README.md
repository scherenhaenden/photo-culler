# Task 36: JPEG burst preselection

This area ports the deterministic burst-selection and legacy CSV behavior from
`photo_culler/selection/preselection.py` and
`photo_culler/cli/commands/preselect_cmd.py`. It accepts already-described JPEG
frames so decoding and metadata extraction can remain in the image/import layer.
It never changes or moves source files.

The core crate does not yet register a `selection` module. Per task ownership,
`src/lib.rs` and `Cargo.toml` are left untouched. The module is self-contained
and can currently be tested with:

```sh
rustc --edition=2024 --test \
  rust/crates/photo-culler-core/src/selection/preselect/mod.rs \
  -o /tmp/photo-culler-preselect-tests
/tmp/photo-culler-preselect-tests
```

Integration should add `pub mod selection;` at the crate root and
`pub mod preselect;` under `selection`, then adapt image metadata output into
`FrameDescriptor`. The byte fixture pins the legacy column order, CRLF records,
CSV quoting, RAW suffix priority, yes/no selection labels, and four-decimal
quality/similarity fields. This module does not read files to derive JPEG
descriptors; a future adapter must preserve the legacy bounded JPEG analysis
and metadata provenance.
