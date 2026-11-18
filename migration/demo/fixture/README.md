# Included demo gallery fixture (D02)

`demo-001.jpg` is a generated synthetic landscape (1024 × 640); it contains no
camera or personal data. `demo-001-thumbnail.jpg` is a distinct 400 × 250 JPEG.
Both files are included so application builds can embed the fixture bytes.
`manifest.json` records their dimensions, byte lengths, provenance and SHA-256.

Regenerate the images with the locked Rust `image` 0.25.10 JPEG encoder:

```sh
cargo run --release --manifest-path migration/demo/fixture/generator/Cargo.toml
python3 migration/demo/fixture/write_manifest.py
```

The JPEGs are encoded with fixed quality settings and dimensions. The first
build resolves the generator's exact `image` version; keep it pinned, then
commit the resulting files and manifest together. Verify hashes with:

```sh
python3 migration/demo/fixture/write_manifest.py
git diff --exit-code -- migration/demo/fixture/manifest.json
```

The API crate's DTO test checks the gallery JSON contract; acceptance also
requires decoding both JPEGs and confirming the thumbnail stays within the
contract edge limit.
