# Task 26 decoder integration notes

`decode_raster` accepts encoded bytes and explicit pixel/allocation limits. The
current workspace codec set is JPEG, PNG, and TIFF. It checks image dimensions
from the decoder header before allocating pixels, applies the decoder's EXIF
orientation, and returns typed failures. The decoder doesn't mutate source
bytes.

RAW decoding is a port boundary: proprietary demosaicing is not included in
this task. The adapter asks `RawPreviewExtractor` for the RAW's embedded
preview first; on extraction/decode failure it asks `PairedRasterFallback`
for the already-associated raster. The catalog/pairing layer owns that
association. The result records which representation supplied the pixels.
Before enabling camera RAW codecs, the integrator should document supported
models/formats, licensing and distribution, and test the approved photo corpus.

The crate root does not yet register `pub mod decode`; integration should add
that module declaration and align `DecodeError`, `DecodedImage`, and
`RepresentationSource` with shared engine DTOs. This area intentionally does
not edit `Cargo.toml` or the root module.

Unit tests are in `mod.rs`. They were compiled through an isolated temporary
Cargo harness (outside the workspace) using the workspace's JPEG, PNG, and
TIFF feature set, so no workspace manifest or lockfile changes were needed.
