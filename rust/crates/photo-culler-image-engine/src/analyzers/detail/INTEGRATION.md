# Detail analyzer integration notes

## Current candidate integration status

The historical note that the crate root did not register analyzer submodules
is stale for the current candidate: `lib.rs` now registers
`analyzers::detail`. Registration only makes this module available.
`NativePixelEngine` continues to compute its existing detail subset through
`metrics::PixelFeatures`; it does not call `DetailAnalysis::analyze` or adapt
`DetailAnalysis::metric_values()` into runtime `MetricResult` values. In
particular, its regional and directional-motion outputs are not runtime
metrics. See the parent `analyzers/INTEGRATION.md` for the audited runtime
boundary and acceptance gates. Keep this API standalone until metric IDs,
units, versions, confidence, and golden-fixture parity are agreed.

## Input contract

`analyze` accepts packed row-major RGB8 bytes (`&[u8]`) and `u32` width and
height after the engine's existing `normalize(image, max_dimension)` step.
This matches the tone analyzer input, so both can read the same decoded and
normalized buffer without copying. Use the same longest-side bound (default
1920) and Rec.709 luminance conversion as `metrics::PixelFeatures`. The module
checks `width * height * 3` with checked arithmetic and verifies that it equals
the byte-slice length before reading pixels. Pass the already-computed value from the existing
`algorithms::fft::fft_high_frequency_ratio` as the final argument; this keeps
one FFT implementation and preserves the current metric contract. Invalid
buffer dimensions return finite zero measurements and `valid_input = false`.
The root `MetricResult` adapter should set `implementation_version` from
`analyzers::detail::IMPLEMENTATION_VERSION` so result caches invalidate when
the detail formulas or units change.

## Output units

- `DetailAnalysis::metric_values()` preserves the current Rust IDs `sharpness.global_score`,
  `sharpness.laplacian_variance`, `sharpness.center_laplacian_variance`,
  `sharpness.effective_focus_variance`, `sharpness.gradient_energy`,
  `sharpness.edge_density`, and `sharpness.fft_high_frequency_ratio`. These map
  to the correspondingly named struct fields (`global_sharpness` and
  `fft_high_freq_ratio` are the two aliases). Variances and gradient energy
  are luminance-levels squared on a 0..255 input; edge density and FFT ratio
  are fractions; global sharpness is clamped to 0..1.
- `regional_laplacian_variance` is a row-major 3x3 array using the same
  luminance-level-squared unit. Cells smaller than 2x2 abstain with zero.
- `DetailAnalysis::metric_values()` preserves the current Rust IDs `noise.luminance_stddev`,
  `noise.chroma_stddev`, `noise.shadow_stddev`, and `noise.estimated_level`.
  Their `*_std` fields are standard deviations in 0..255 luminance/channel
  levels; `estimated_noise_level` is a bounded 0..1 score; flags retain the
  legacy 0.35 and 0.12 thresholds.
- `blur_direction_deg` is the dominant gradient orientation, not its
  perpendicular, wrapped to [0, 180). `blur_length_px` uses normalized-frame
  pixels. Anisotropy and confidence are fractions in 0..1.
- Motion values are exposed under `motion_blur.*` IDs by `DetailAnalysis::metric_values()`;
  `is_directional_motion_blur` remains a boolean field rather than a scalar
  `MetricResult`.

The module reuses the existing Rust edge and noise algorithms. Synthetic tests
cover focused versus blurred texture, clean versus noisy frames, directional
versus isotropic detail, and finite values for empty, small, uniform, and
malformed inputs.

Standalone acceptance command from the repository root:

```sh
rustc --edition=2024 --test rust/crates/photo-culler-image-engine/src/analyzers/detail/mod.rs \
  -o /private/tmp/photo-culler-task30-detail-tests
/private/tmp/photo-culler-task30-detail-tests
```
