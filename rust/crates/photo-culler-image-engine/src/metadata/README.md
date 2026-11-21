# Photo metadata

`normalize` accepts already parsed `TagRecord` values and preserves winning
field provenance and the original value. The only container parser currently
implemented is for JPEG APP1 segments carrying a TIFF/EXIF header. It recognizes
these EXIF IFD tags:

- IFD0: `Make` (0x010F), `Model` (0x0110), `Orientation` (0x0112), `DateTime` (0x0132).
- Exif IFD: `ExposureTime` (0x829A), `FNumber` (0x829D),
  `PhotographicSensitivity` / ISO (0x8827), `DateTimeOriginal` (0x9003),
  `CreateDate` (0x9004), `OffsetTimeOriginal` (0x9011), `OffsetTime` (0x9012),
  and `FocalLength` (0x920A).

Call `read_jpeg_file(path)` to read a bounded JPEG header without loading image
pixels, `read_jpeg_metadata(bytes)` to parse and normalize a byte slice, or
`parse_jpeg_exif(bytes)` when the caller needs the source-tag records. Inputs
without an EXIF APP1 segment return metadata with absent fields. Malformed or
oversized structures return a stable `ExifReadError`. Invalid typed values
that reach normalization are omitted and reported through
`PhotoMetadata::warnings`; blank Make/Model strings are simply left absent.

The parser reads at most the first 16 MiB from a file and caps the JPEG segment
count at 4,096, TIFF IFD depth at 4, entries at 4,096 total, and one referenced
tag payload at 1 MiB. It validates offsets and lengths before slicing data.
`ExifReadError` distinguishes non-JPEG input, truncated input, malformed EXIF,
resource limits and file I/O. TIFF byte order and inline/out-of-line values for
ASCII, SHORT, LONG and unsigned RATIONAL fields are handled. `DateTimeOriginal`
is preferred over generic `DateTime`/`CreateDate`, and its offset is preferred
over `OffsetTime` when both exist. Without a valid timezone offset, capture time
retains its camera-local text and does not invent UTC.

The JPEG reader stops walking markers at the start of the image scan. It does
not validate entropy-coded image data, require a complete scan, or establish
that the JPEG pixels are decodable; use the image decoder for that. The public
byte-slice parser is bounded by segment, IFD, and tag limits, while the file
adapter additionally caps its read at 16 MiB.

This reader does **not** parse standalone TIFF files, RAW containers or maker
notes, and it does not read XMP/JSON sidecars. `Source::RawEmbedded` and
`Source::Sidecar` remain normalization inputs for readers that may be integrated
later. Do not use this JPEG subset as evidence that RAW metadata support is
complete.
