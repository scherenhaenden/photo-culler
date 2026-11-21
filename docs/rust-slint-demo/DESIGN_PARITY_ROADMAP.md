# Photo Culler Pro design port roadmap

The user-provided `photo_culler_pro.zip` is the visual source of truth for the Rust/Slint UI. Its SHA-256 and the exact initial Library layout are recorded in [Photo Culler Pro design spec](PHOTO_CULLER_PRO_DESIGN_SPEC.md). The ZIP contains React/Tailwind source but no screenshots; use the component code as the specification, and keep React/Gemini out of the shipped application.

The earlier implementation at [`3701bed`](https://github.com/scherenhaenden/photo-culler/commit/3701bedf924bc3a3927166ef3a86c59d688d1ab3) remains useful for legacy labels, behavior and migration checks. It does not override the Pro package's screen composition. In particular, the Pro Library is a grid with a separate primary-navigation rail and filter rail; it is not a preview/filmstrip/inspector hybrid. Cinema is a separate screen with an inspector.

## Sprint 01 gate: the Library screen

| Surface | Layout contract | First comparison |
|---|---|---|
| Native Slint | 46 px top header; 220 px primary navigation; 230 px Library filters; flexible toolbar and photo grid. | Capture at 1440 × 900 logical pixels, with the same fixture, selected photo, locale and scale as the prototype. |
| Slint WebAssembly | Same Library layout and controls in the browser; preserve the app bootstrap diagnostic outside the Slint canvas. | Capture at 1440 × 900 and a constrained width; check grid density and whether the filter rail follows the reference. |

The Library view must not contain the Cinema inspector. It needs the prototype's top navigation/actions, primary sidebar, Library-only filter rail, 46 px grid toolbar and 4:3 cards with cover crops, decision/rating metadata and selection treatment. Keep counts and state labels tied to Rust data, not hard-coded to the screenshot.

## Current status for the initial Library screen

At code SHA `afe088242bdcf173eb8e83c9571bf598f52c4e86`, UX01 has paired,
hash-identified reference/candidate captures at 1440×900 and 1100×700. The
candidate has responsive 5×2 and 4×3 grids and local contract coverage for the
smoke fixture. The native Library window was also accepted on Linux, macOS and
Windows by run [#44546749693](https://github.com/scherenhaenden/photo-culler/actions/runs/44546749693),
on a candidate based on this S05 code. UX02–UX04 are implemented in part: the
main geometry and basic filters are present, but several control details still
differ and the live REST response does not include Pro metadata. Vector icon
paths are visually checked on the separate stacked PR #155 branch, but are not
in the S05 base capture. See the [paired-image review](evidence/design/README.md)
for the observed deltas. Remaining UX02–UX04 acceptance covers these visual
details, live metadata, and state behavior.

## Remaining design milestones

| ID | Milestone | Work | Acceptance |
|---|---|---|---|
| UX01 | Pin and capture the reference | **Complete for the initial state.** Paired deterministic captures at 1440×900 and 1100×700 use the same local JPEG and identify source ZIP, fixture and hashes. | Reproduce the paired captures from the recorded command and keep them versioned with the manifest. |
| UX02 | Port the shared shell | **Partial.** Header, tabs, 220 px navigation, counters and footer are present. Vector icons are visually verified on PR #155; the S05 base still uses placeholders, and some spacing/typography remain. | Geometry, labels, icon states, color roles and active/inactive states match the package. |
| UX03 | Port Library filters and grid | **Partial.** Separate 230 px filter rail, toolbar, responsive 4:3 grid, filter behaviors and selection are present. Some control affordances and icon details differ. | Matched captures show the same columns, crop, selected ring, badges, hover overlay, counts and filter controls. |
| UX04 | Connect the live Sprint 01 controller | **Partial.** Shared REST data and same-origin WASM are connected. Pro metadata appears only in the explicit synthetic fixture; missing live fields remain unavailable. | The real Rust API thumbnail appears in the reference card layout; loading, empty, error and retry preserve the shell without fabricating metadata. |
| UX05 | Port Cinema and Compare | Implement the package's separate Cinema viewer/inspector and synchronized 4-up comparison. | Each screen uses its own reference layout and supports its defined selection/navigation behavior. |
| UX06 | Port Bursts, Timeline, Dashboard and dialogs | Port the remaining navigation destinations and import/export/settings/shortcuts surfaces, then connect operations as their Rust endpoints exist. | Layout matches the source; unavailable operations are explicit and recoverable. |
| UX07 | Verify full design parity | Capture each view on native and web and record geometry, typography, cropping, focus, responsive behavior and platform differences. | Each screen has a reviewed pair and documented resolution or approval for every material difference. |

UX01–UX04 are the Sprint 01 design gate. UX05–UX07 belong to later migration stages and are not implied by a passing one-photo demo. See [Sprint 01 remaining work](SPRINT_01_REMAINING.md) and the [64-task Rust/Slint roadmap](../RUST_SLINT_ROADMAP.md#ensamblaje-y-entrega).
