# Demo REST contract (D01)

The versioned API is rooted at `/api/v1` and is read-only for this demo. The
canonical Rust DTOs and thumbnail URL builder live in
`rust/crates/photo-culler-api`. Server and client adapters must use that crate
instead of rebuilding the route string.

* `GET /api/v1/health` returns `status`, `resources`, and `request_id`.
* `GET /api/v1/galleries/{gallery_id}/photos` returns `gallery_id`, `items`,
  `next_cursor`, and `total`; an empty result has an empty array, null cursor,
  and total zero. Each photo includes `photo_id`, `filename`, `representation`
  (`jpeg` or `raw`), `decision`, and `thumbnail_url`.
* `GET /api/v1/photos/{photo_id}/thumbnail/{max_edge}` returns JPEG bytes with
  `image/jpeg`, or the typed `code` / `message` / `request_id` error envelope.
* Page sizes default to 100 and are capped at 500. Thumbnail edges are in the
  inclusive range 1–2048 pixels. Clients should use `thumbnail_path` to build
  the relative route; identifiers are encoded as one path segment. The encoded
  thumbnail response is limited to 2 MiB.
* Error messages are safe, user-facing text and must not disclose local paths.

Examples are under `examples/`. At the base commit there was no Rust API adapter
to preserve; existing application behavior remains a compatibility reference
and adapters are still required to translate it to these versioned DTOs.
