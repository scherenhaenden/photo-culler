# Tone analyzer integration

This folder provides a pure Rust analysis module over an orientation-corrected,
RGB8 row-major buffer that has already been bounded to the shared analysis
resolution (currently longest side 1920 px). It does not read or write source
files. The decoder owns file-level decodability and corruption status; an invalid
pixel buffer returns `ToneInputError` rather than being reported as a decoded
image.

## Current candidate integration status

The historical note for base `1871eb9` is no longer an instruction for the
current candidate: `lib.rs` now registers `analyzers::tone`. Registration only
makes the module available. `NativePixelEngine` still emits its existing
`metrics::PixelFeatures` values with version `rust-pixels-0.1`; it does not call
`ToneAnalysis::analyze` or adapt `ToneAnalysis::metric_values()` into
`MetricResult`. See the parent `analyzers/INTEGRATION.md` for the audited
runtime boundary and calibration gates. Keep the tone outputs standalone until
the shared IDs, units, confidence policy, and golden-fixture parity are agreed.

## Metric contract

Analyzer groups retain the IDs `dimensions`, `histogram`, `exposure`, and
`clipping`. Numeric IDs emitted by `metric_values()` include dimensions in
pixels and megapixels, aspect ratio, luminance statistics and percentiles in
8-bit scale (0–255), histogram entropy in bits, exposure score/probabilities in
0–1, and clipping ratios in 0–1. The Rust shadow adapter's existing IDs
`histogram.luminance_stddev`, `histogram.luminance_entropy`,
`clipping.*_ratio`, and `exposure.*` are preserved. The Python clipping analyzer
uses `*_pct` values in 0–100; multiply Rust ratios by 100 when adapting to that
legacy shape. Do not relabel a fraction as a percentage.

The tone analyzer always retains measured global and center clipping. Concert
lights remain visible in the raw ratios; the existing `concert` behavior is a
scoring policy that reduces the weight of global highlight clipping and gives
more weight to center clipping. Apply that policy downstream rather than
suppressing or rewriting measurements here.

The caller must supply the same normalized pixels to tone and detail analyzers.
Orientation, dimensions, and normalization are resolved before calling this
module. Empty images and empty central regions return finite zero values. A
buffer whose length does not equal `width * height * 3` is rejected.

## Standalone tests

```sh
rustc --edition=2024 --test rust/crates/photo-culler-image-engine/src/analyzers/tone/mod.rs -o /private/tmp/photo-culler-task29-tone-tests
/private/tmp/photo-culler-task29-tone-tests
```

The tests cover black pixels, clipped highlights, stage-light measurements,
dimensions and metric units, incomplete buffers, and finite results for empty,
small, and uniform samples.
