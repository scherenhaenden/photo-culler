# Recipe rendering — task 39

This isolated module provides an injected decoder, in-memory before/after preview, a bounded exposure and white-balance recipe, separate focus/noise/clipping/recoverability warnings, a RawTherapee PP3 suggestion, and an explicit derived-file writer. The PPM encoder is a portable test/output adapter; production should inject the image-engine decoder and chosen derivative encoder.

Run its standalone unit tests with:

```sh
rust/crates/photo-culler-image-engine/src/editing/render/test-standalone.sh --offline
```

The tests use a fake decoder and golden RGB/PP3 values. They compare SHA-256 of immutable source bytes before rendering and after an explicit derivative write. The temporary harness avoids changing crate manifests or root modules, which are integrator-owned.

## Integration

Register `editing::render` from the image-engine crate root and add it to the shared workspace only during assembly. Replace the fake/test decoder with the supported JPEG/RAW decoder, adapt the image engine's actual pixel buffer and color profile, agree on a versioned recipe DTO with the server recipe-history task, and verify RawTherapee PP3 compatibility against an approved golden profile. Keep derivatives on an explicit destination and run the original-hash check with real photo fixtures. This task intentionally does not alter manifests, crate roots, or workflows.
