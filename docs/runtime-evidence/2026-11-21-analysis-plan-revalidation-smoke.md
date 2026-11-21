# D25 analysis plan revalidation runtime smoke

**Date:** 2026-11-21
**Candidate base:** `26991eb0f564f42fd47fa529e1f5fd2320dc5b27`
**Candidate Rust manifest:** 165 non-Markdown files, SHA-256
`251dcf4998779e3f9739517c1be3c87b56c5375de75cad98ae568bb1b1a4ed44`
**Host:** macOS arm64, server bound to `127.0.0.1`
**Scope:** manual API acceptance for queued-plan invalidation and recovered
paused-plan invalidation. No automated test suite was run.

## Artifacts and setup

The smoke reused the already-built shared server binary; it did not compile:

```text
/private/tmp/photo-culler-d25-smoke-target/debug/photo-culler-server
SHA-256 9068852e88c7ed4432f1595b3b860b96ebbaf552b25abaa8b1c7b1d4e3b6a4de
```

The independent fixture was a copy of
`migration/demo/fixture/demo-001.jpg` (40,001 bytes; SHA-256
`f996f797fde19c10187d19538ad738a18235c6a9b03d34b85b896cff61401922`). All
fixture copies and SQLite data were created in unique `/private/tmp` folders.
The owned server was launched with:

```sh
/private/tmp/photo-culler-d25-smoke-target/debug/photo-culler-server \
  --owned --port 0 --data-dir "$DATA" --authorized-root "$ROOT"
```

The ready JSON supplied an ephemeral `http://127.0.0.1:<port>` origin and
Editor bearer token. Requests below used both `Authorization: Bearer $TOKEN`
and `Origin: $ORIGIN`; the token and ephemeral port are intentionally omitted.

## Queued job, catalog change, Start rejection

1. Copied the JPEG to `$ROOT/a.jpg`; started the server and selected the
   bootstrapped gallery and its filesystem source using
   `GET /api/v1/galleries` and
   `GET /api/v2/galleries/{gallery_id}/sources`.
2. Called
   `POST /api/v2/galleries/{gallery_id}/imports` with
   `{"source_id":"<source_id>","recursive":true}`. The import completed
   with `discovered=1`, `imported=1`.
3. Submitted
   `POST /api/v1/galleries/{gallery_id}/analysis-jobs` with
   `{"job_id":"d25-plan-change","profile_id":"technical"}`. It returned
   HTTP 201, `state=queued`, `total=1`, `completed=0`.
4. Copied the same fixture bytes to `$ROOT/b.jpg` and repeated the import
   request. It completed with `discovered=2`, `imported=2`.
5. Called
   `POST /api/v1/analysis-jobs/{catalog_id}/d25-plan-change/start` with `{}`.
   It returned HTTP 422 and
   `{"error":{"code":"invalid_plan","message":"The gallery does not have an analyzable plan."}}`.
   A status read immediately before Start still showed `queued`, 0/1 complete.

This demonstrates that a queued plan cannot start after reimport changes its
ordered photo set.

## Paused recovery, catalog change, Resume rejection

1. In a separate isolated fixture, made 40 byte-identical copies of the JPEG
   under distinct filenames and imported them (40-photo analysis plan).
2. Submitted
   `POST /api/v1/galleries/{gallery_id}/analysis-jobs` with
   `{"job_id":"d25-recovery-change","profile_id":"technical"}`; the
   queued plan reported `total=40`.
3. Issued Start, then
   `POST /api/v1/analysis-jobs/{catalog_id}/d25-recovery-change/pause` with
   `{}`. Pause returned HTTP 200. The job reached `paused` at 1/40 before the
   server was stopped.
4. Restarted the same binary with the same `--data-dir` and `--authorized-root`.
   The recovered job remained `paused`, 1/40 complete.
5. Added one more fixture copy and reimported. The import completed with
   `discovered=41`, `imported=41`.
6. Called
   `POST /api/v1/analysis-jobs/{catalog_id}/d25-recovery-change/resume` with
   `{}`. It returned HTTP 422 with error code `invalid_plan`.

## Cleanup and limits

Both servers were stopped and all smoke-owned fixture, database, WAL/SHM, and
temporary log data were removed. The shared build target was left untouched.
`df -h /private/tmp` after cleanup reported 57 GiB available. The shared build
target was left untouched.

This validates only the manual loopback API behavior for one queued plan and
one recovered paused plan using synthetic JPEGs on macOS arm64. It does not
cover UI interactions, other platforms, a real photo catalog, RAW decoding,
large-catalog performance, or release acceptance.
