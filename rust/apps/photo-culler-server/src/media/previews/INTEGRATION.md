# Preview module integration status

The bounded preview engine is registered as `media::previews` in the server.
The persistent server exposes a read-only JPEG endpoint through
`http::previews` and `http::previews_sqlite`; see
[`http/previews/INTEGRATION.md`](../../http/previews/INTEGRATION.md) for routes,
limits, cache behavior, and acceptance gaps.

The current SQLite adapter resolves the preferred present source through the
catalog, hashes the exact source bytes, applies decoder orientation, and
creates a bounded JPEG derivative. Decoded pixels are capped at 16 million,
with a 128 MiB decoder allocation limit. RGB conversion, resizing, and encoding
allocate additional buffers, so these limits are not a total process-memory
ceiling. At most two generations run concurrently.

The typed `PreviewClient` now supplies gallery images on persistent servers;
fixture servers retain their legacy thumbnail fallback. The recipe UI does not
request previews. A limited synthetic JPEG smoke passed, but the route has no
full runtime acceptance; embedded RAW extraction, paired raster fallback, and
edited-recipe before/after rendering remain open. See the
[runtime evidence](../../../../../../docs/runtime-evidence/2026-11-21-synthetic-persistent-smoke.md).
Keep the service loopback-only until session authentication is integrated.
