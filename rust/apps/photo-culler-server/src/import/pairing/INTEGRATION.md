# Pairing integration note (task 22)

This source directory is prepared for the planned `photo-culler-server` crate.
At base commit `a68f360`, that crate, its crate manifest, root module tree, and
the shared integration harness do not exist yet. This directory therefore
contains a standalone Rust module and injected-record unit tests; the server
module declaration and scanner/catalog adapters remain pending integrator work.

The caller should pass scanner records with source-root-relative paths and
stable existing file IDs. Pairing never creates or changes a `PhotoId`; it
returns association groups only. Path matching uses a lower-cased parent and
stem for compatibility with the legacy pairer's case-insensitive grouping.
Files with the same stem in different directories remain distinct. Duplicate
RAW, JPEG, or other-raster candidates of the same kind are marked ambiguous.
No candidate is selected by order or extension; sidecars for ambiguous media
stay unattached with an explicit reason.

When the crate is registered, add `pub mod pairing;` at the server import module
and run this module's tests through the shared workspace harness. No crate
manifest or root module was added here because those paths are outside task 22's
exclusive area.
