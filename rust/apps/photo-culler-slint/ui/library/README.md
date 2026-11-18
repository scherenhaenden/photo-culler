# Task 45: bounded library grid

The sprint base does not yet contain the Slint application, shared UI
components, REST client, or catalog read routes. This folder provides the
transport-independent state contract for that screen. It does not render UI,
decode thumbnails, or fetch network data by itself.

`LibrarySource` is the adapter boundary. A REST implementation should request
only `PageRequest.limit` rows from the catalog endpoint and apply the supplied
query, session, and decision filters on the server. `LibraryGrid` clamps page
size to 100 and thumbnail requests to 32 photo IDs at a time. The Slint view
should render the current page and ask for thumbnail IDs for its visible range
plus a small preload window; it must not create a request per gallery item.

Navigation keys move focus across rows and columns; Enter records the active
photo ID. Changing a JPEG/RAW representation updates the active row while
keeping that same photo ID. Applying a filter resets to page zero. Page order
is the stable order supplied by the source; the REST adapter should request the
catalog's stable sort and cursor contract when it is available.

When integrating, add `ui::library` to the Slint app and map the view callbacks
to this state model. Keep thumbnail fetching asynchronous and bounded, and
ignore late responses whose photo ID or representation no longer matches the
visible row. No manifest or crate-root changes are made here because those
files are outside this task's ownership and the app crate does not yet exist.

Run the self-contained acceptance tests with:

```sh
rustc --edition=2024 --test \
  rust/apps/photo-culler-slint/ui/library/mod.rs \
  -o /tmp/photo-culler-task45-tests
/tmp/photo-culler-task45-tests
```
