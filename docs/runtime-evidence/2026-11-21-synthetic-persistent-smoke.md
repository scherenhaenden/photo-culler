# Synthetic persistent-catalog runtime smoke

**Date:** 2026-11-21
**Candidate base:** `26991eb` (`feature/sprint-01-integration-candidate`)
**Host:** macOS arm64, local loopback listener
**Scope:** API smoke only; not UI, release, or cross-platform acceptance.

## Candidate identity

The candidate was an uncommitted worktree. Its non-Markdown Rust workspace
manifest contained 164 files and had SHA-256
`b7a0e4848767849ba4aa1fcd069464e67dc43e47127541072b010ebc89a3e49f`.
The manifest is reproducible by sorting regular files below `rust/` except
Markdown files and hashing the concatenation of each relative path, a NUL byte,
and that file's SHA-256 digest. Markdown documentation edits do not alter this
identity. This identifies the source snapshot used for the smoke without
implying that it was a commit or a release build.

## Setup and results

The server was built and run with:

```sh
CARGO_TARGET_DIR=/private/tmp/photo-culler-runtime-elevated-target \
cargo run --locked -p photo-culler-server -- \
  --port 18765 \
  --data-dir /private/tmp/photo-culler-runtime-elevated-data \
  --authorized-root /private/tmp/photo-culler-review/migration/demo/fixture
```

The only authorized source was the repository's two-file synthetic JPEG
fixture. SQLite data and generated previews were isolated under `/private/tmp`.
The temporary target and database were deleted after the run, restoring
`/private/tmp` to 58 GiB free.
The first POST without an `Origin` header received `403`; retrying with
`Origin: http://127.0.0.1:18765` produced:

- `GET /api/v1/health`: ready.
- `GET /api/v1/galleries` and `GET /api/v2/galleries/{gallery_id}/sources`:
  one catalog gallery and one online fixture source.
- `POST /api/v2/import-estimates`: 2 files, 48,305 bytes, 2 logical photos.
- `POST /api/v2/galleries/{gallery_id}/imports`: queued; the job completed
  with 2 discovered/imported, 0 issues.
- `GET /api/v1/galleries/{gallery_id}/photos?limit=12`: returned both photos.
- `GET /api/v1/photos/{photo_id}/preview?size=medium`: HTTP 200,
  `image/jpeg`, 24,557 bytes, decoded dimensions 800×500.
- `PUT /api/v1/photos/{photo_id}/decision`, expected revision 0: returned
  decision `keep`, revision 1. The updated page showed the persisted decision.

The source fixture SHA-256 in base commit `26991eb` and after the runtime smoke
was `f996f797fde19c10187d19538ad738a18235c6a9b03d34b85b896cff61401922`.
During the smoke, the file was also hashed after preview/decision and after
analysis; both values matched. The fixture remains clean in the worktree.

The server was stopped normally and restarted against the same data directory.
Health returned ready; the imported photos, completed job and `keep` decision
were still present, and preview again returned an 800×500 JPEG.

After restart, the D25 API accepted a `technical` analysis job for the two
photos. It reached `succeeded` with 2/2 completed and 0 failed; the photo DTO
exposed metrics under `analysis_summary.rust_shadow`.

## Limits

This smoke proves a small persistent API path on one macOS host against bundled
synthetic JPEGs. It does not verify large catalogs, RAW/sidecars, active-job
recovery, desktop or browser UI behavior, color-managed edited previews,
export, authentication, NAS, or Linux/Windows/WebAssembly runtime acceptance.
The API remains unauthenticated and must stay loopback-only. No automated test
suite was run as part of this smoke.
