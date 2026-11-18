# Task 12: catalog read integration

This is a transport-independent provisional adapter, because commit `a68f360`
has neither the new server crate nor the frozen common HTTP/identity contracts.
It is not registered as HTTP routes and does not claim conformance with the
future agreed DTOs. Detail and summary shapes use the legacy Python v1 API as
an explicit reference; the cursor page, active gallery and profile DTOs are
provisional. No external request can reach these functions until integration.

The integrator should add `mod catalog_reads` to the server, connect a `Catalog`
implementation to domain services and mount authenticated read-only Axum GET
handlers for galleries, active gallery, catalog, photo detail, sessions, groups,
profiles, summary and thumbnail. Serialize DTOs with `serde`/`serde_json`; map
`NotFound` to 404, invalid page/cursor mismatch to 400, and `Unavailable` to
503 using the shared `code/message/request_id` error contract. Return thumbnail
bytes and their validated media type in the HTTP response. The media service
must enforce source permissions and return only generated derivatives, not paths.
The session and origin middleware from tasks 10/11 must wrap the router.

Before integration, replace the provisional types with the frozen identity and
DTO types, agree URI names, encode the cursor as an opaque client token, bind it
to all filters, and confirm the thumbnail URI against the frozen contract. IDs must be
unique and immutable. The current keyset order is ascending photo ID; with a
changing catalog, inserts before the cursor do not appear in an ongoing walk.
For a consistent snapshot, the domain port must supply snapshot/revision
semantics. This standalone implementation loads the filtered set before paging;
the production port should move the same ordering and keyset predicate into the
repository to avoid materializing large catalogs. The adapter does not verify
that a requested gallery exists: the repository must return `NotFound` for an
unknown gallery and distinguish that from an empty gallery.

Standalone test without changing the repository manifests: create a temporary
Cargo package with dependencies `serde` (derive) and `serde_json`, set its
`[lib] path` to this `mod.rs`, and run `cargo test`. These tests exercise the
adapter with a two-gallery fake; they do not exercise a real HTTP stack, media
store, or the not-yet-frozen DTO contract.
