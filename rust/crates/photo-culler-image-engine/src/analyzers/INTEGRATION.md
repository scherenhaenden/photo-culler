# D29–D32 analyzer and scoring integration status

**Candidate base:** `26991eb`; all statements below describe source wiring, not
runtime or score-calibration acceptance.

## Runtime path that is connected

`photo-culler-server` uses `jobs::analysis::ImageEngineAnalyzer`, which calls
`photo_culler_image_engine::NativePixelEngine`. That adapter decodes a raster
path, normalizes its longest side to at most 1920 pixels, computes
`metrics::PixelFeatures`, and emits a vector of `MetricResult` values with
implementation version `rust-pixels-0.1` and confidence `1.0`. The analysis
repository persists those metric values in
`photos.analysis_summary_json.rust_shadow`; it does not publish or overwrite
the Python-selected result, quality tier, or decision.

The currently emitted IDs and units are:

| IDs | Units / range |
| --- | --- |
| `dimensions.width`, `dimensions.height` | pixels |
| `exposure.mean_luminance`, `histogram.luminance_stddev` | 8-bit luminance levels (0–255) |
| `histogram.luminance_entropy` | bits |
| `exposure.score`, `exposure.underexposed_probability`, `exposure.overexposed_probability` | fractions / scores in 0–1 |
| `clipping.*_ratio` | fractions in 0–1; legacy Python percentage fields require ×100 |
| `noise.luminance_stddev`, `noise.chroma_stddev`, `noise.shadow_stddev` | 8-bit levels |
| `noise.estimated_level`, `sharpness.global_score`, `sharpness.edge_density`, `sharpness.fft_high_frequency_ratio` | bounded score/ratio in 0–1 |
| `sharpness.gradient_energy`, `sharpness.laplacian_variance`, `sharpness.center_laplacian_variance`, `sharpness.effective_focus_variance` | luminance-level squared on the normalized frame |

Decode failures return analyzer errors and become failed-photo outcomes. The
current metric vector does not include a scalar integrity/corruption metric,
RAW recoverability, camera metadata, regional sharpness, or a semantic subject
estimate.

## Modules present but not called by the runtime adapter

- `analyzers::tone` is registered in `lib.rs` and has a pure RGB8 API plus
  `ToneAnalysis::metric_values()` and version `rust-tone-0.1`. It exposes richer
  dimensions, luminance percentiles, histogram channels, per-channel clipping,
  and stable analyzer IDs. `NativePixelEngine` currently calls
  `PixelFeatures::from_image` instead; it does not call the tone analyzer or
  adapt its `metric_values()`.
- `analyzers::detail` is registered and exposes region-level sharpness, noise,
  and directional-motion estimates with version `rust-detail-0.1`. The runtime
  path uses lower-level noise/edge/FFT algorithms through `PixelFeatures`, not
  `DetailAnalysis::analyze` or its metric adapter. Directional motion and the
  3×3 regional measurements therefore are not emitted by runtime analysis.
- `analyzers::composition` is registered and exposes horizon and visual-balance
  estimates with confidence and normalized coordinates. No `MetricResult`
  adapter or runtime invocation connects it to D25; its suggestions are not
  scores and do not change crops or source pixels.

Before consolidating these parallel paths, define one versioned metric schema
and compare duplicate IDs and numeric values against the required golden
fixtures. Until then, do not count the standalone modules as connected runtime
features or silently replace the existing `rust-pixels-0.1` result contract.

## D32 scoring boundary

`photo_culler_core::scoring` is registered and contains versioned `fast`,
`technical`, and `concert` profiles, metric-version cache keys, and component
explanations. Repository-wide call-site search finds no production invocation
of `scoring::score`; the only calls are inside that module's unit-test source.
`ImageEngineAnalyzer` serializes metric values, and the SQLite repository stores
only those values under `rust_shadow`.

The profile weights and tier thresholds currently lack a calibrated real-photo
golden corpus or comparison against the selected Python pipeline. Concert-light
behavior is represented in the provisional formula but has not passed
fixture-based calibration. Recoverability is only copied from an optional input
as raw evidence; this scorer does not calculate recoverability or apply a
recovery penalty. The scorer also accepts metrics individually and renormalizes
missing weights; runtime policy has not defined a minimum required metric set
for publishing a score. Therefore the score, quality tier, and recoverability
result must remain out of the production review/decision path until calibration
and schema integration are accepted.

## Acceptance still needed

- Compare D29 values with black, clipped-highlight, concert-light, malformed,
  and empty-image goldens, including units and legacy identifiers.
- Compare D30 focus/noise/motion ordering and finite/abstention behavior on
  normalized fixtures; wire regional and motion values into a versioned result
  schema only after parity review.
- Verify D31 angle direction, confidence abstention, normalized endpoints, and
  balance behavior on horizon/curved/fisheye/no-subject fixtures.
- Calibrate D32 profile weights, thresholds, recoverability, and explanations
  against labeled authorized photo fixtures and Python shadow outputs; record
  confidence, metric versions, and minimum data requirements.
- Run those checks on the candidate SHA and then perform runtime acceptance on
  an authorized corpus. No test suite or calibrated score acceptance is claimed
  by this source audit.
