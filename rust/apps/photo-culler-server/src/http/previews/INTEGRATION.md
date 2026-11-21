# Read-only preview endpoint

`http::previews::router()` serves `GET /api/v1/photos/{photo_id}/preview`.
The optional `size` query accepts `small`, `medium`, `large`, or `full` and
defaults to `medium`. The response is a JPEG derivative; no source path or
filesystem error is returned. `http::previews_sqlite::SqlitePreviewApi`
resolves the preferred present source through `Catalog::display_path`, reads
the bytes without modifying them, and keys its bounded in-memory cache by the
SHA-256 of those exact bytes, photo identity, render version, and size.

Input is limited to 128 MiB, decoded pixels to 16 million, decoder allocation
to 128 MiB, output to 16 MiB, and cache storage to 256 MiB. At most two
generations may run concurrently; excess work returns `429`. These decoder
limits do not equal total process memory: RGB conversion, resizing, and JPEG
encoding use additional working buffers. EXIF orientation is applied by the
image decoder before bounded resizing. Unsupported RAW data returns a typed
`422`; embedded RAW preview extraction and paired-raster fallback are not wired
to the catalog in this adapter.

The route, SQLite adapter and `media::previews` are registered in `lib.rs` and
mounted in both persistent catalog routers. The typed `PreviewClient` is used
by `DemoRestClient` for gallery images on persistent servers; a 404 falls back
to the legacy thumbnail route used by fixture servers. The recipe panel does
not request previews, so edited before/after rendering is still unavailable.
The loopback-only service still lacks session authentication; do not expose
this route on a LAN.

A limited runtime smoke exercised one 800×500 JPEG preview after import and
again after a server restart; the source fixture hash matched the base commit.
See the [runtime evidence](../../../../../../docs/runtime-evidence/2026-11-21-synthetic-persistent-smoke.md).
This does not cover the full preview contract. Verify EXIF orientation,
unsupported/truncated files, cache invalidation on changed bytes, high-size
requests, and concurrent request limits before promoting the route from
candidate status.
