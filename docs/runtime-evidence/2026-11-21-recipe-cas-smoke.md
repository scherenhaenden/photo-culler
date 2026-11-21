# D38 recipe API runtime smoke

**Date:** 2026-11-21 (UTC)
**Candidate:** uncommitted worktree based on `26991eb`
**Rust input manifest:** 164 non-Markdown files, SHA-256 `5c2402e3d9ffc8f36eae4721950cd41e552dacacb8f6f54323d5fd3fd4c124bf`
**Host:** Darwin arm64; server bound only to `127.0.0.1:18766`
**Data:** isolated SQLite directory `/private/tmp/photo-culler-recipe-runtime-data`; authorized synthetic fixture `migration/demo/fixture`.

## Run

Started with:

```sh
CARGO_TARGET_DIR=/private/tmp/photo-culler-recipe-runtime-target \
cargo run --locked --manifest-path rust/Cargo.toml -p photo-culler-server -- \
  --bind 127.0.0.1 --port 18766 \
  --data-dir /private/tmp/photo-culler-recipe-runtime-data \
  --authorized-root /private/tmp/photo-culler-review/migration/demo/fixture
```

The import estimate reported 2 files, 48,305 bytes, 2 logical photos, both JPEG. Import completed with `discovered=2`, `imported=2`, `issues=0`. The fixture's `demo-001.jpg` SHA-256 was `f996f797fde19c10187d19538ad738a18235c6a9b03d34b85b896cff61401922`; the fixture was not modified.

For photo `631e852c0c525efe8657127bbcbb60b4`:

1. GET recipe returned revision 0 and defaults: exposure 0, temperature 6500, tint 0.
2. PUT with `expected_revision=0`, exposure 0.75, temperature 6100, tint 4 returned HTTP 200 at revision 1.
3. A second PUT using stale `expected_revision=0` returned HTTP 409 `version_conflict`, `actual_revision=1`.
4. Undo with expected revision 1 returned HTTP 200 at revision 2, defaults restored, `can_redo=true`.
5. Redo with expected revision 2 returned HTTP 200 at revision 3 and restored the saved values.
6. Stopped and restarted the same server against the same data directory. GET returned revision 3 with the saved values, `can_undo=true`, `can_redo=false`.

Mutation requests included `Origin: http://127.0.0.1:18766`. The API echoes `request_id`, but this smoke does not establish request deduplication/idempotency.

## Scope

This is runtime evidence for the D38 recipe API and SQLite state/history only. It does not accept the Slint recipe panel, before/after renderer, color fidelity, export, authentication, RAW handling, performance, or any non-macOS target. No test suite was run. Temporary runtime target and data were removed after capture; the pre-existing main checkout was not modified.
