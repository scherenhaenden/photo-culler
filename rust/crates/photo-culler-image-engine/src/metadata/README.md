# Task 27 metadata adapter

This module normalizes parser output into per-field values with source, tag,
and original text. It is deliberately independent of the image container
reader because this base has no EXIF/RAW/sidecar parser API and changes to
Cargo manifests and root module registration belong to integration.

The decoder and sidecar readers should adapt their parsed records to
`TagRecord`, labeling them `JpegExif`, `RawEmbedded`, or `Sidecar`. Sidecars
have higher precedence than embedded records; for equal source precedence,
callers must provide stable tag order. Missing fields remain `None`. A capture
timestamp without a valid offset retains its local original string and has no
UTC instant; this avoids assuming a machine timezone. Invalid values remain
absent and produce a warning with a stable code.

Integration points:

- Register `pub mod metadata;` in the image-engine crate root when assembling
  the task packages.
- The decoder should supply TIFF/EXIF tag names and values without discarding
  the container source; XMP/JSON sidecar adapters should map the supported
  aliases to these tag names.
- Agree the public Serde/JSON DTO and normalization policy in the shared API
  contract. This task does not modify Cargo.toml or lib.rs.

Standalone acceptance command:

```sh
rustc --edition=2024 --test rust/crates/photo-culler-image-engine/src/metadata/mod.rs -o /tmp/task27-metadata-tests
/tmp/task27-metadata-tests
```
