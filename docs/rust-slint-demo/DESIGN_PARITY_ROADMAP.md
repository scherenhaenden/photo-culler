# Photo Culler Pro design port roadmap

The user-provided `photo_culler_pro.zip` is the visual source of truth for the Rust/Slint UI. Its SHA-256 and the exact initial Library layout are recorded in [Photo Culler Pro design spec](PHOTO_CULLER_PRO_DESIGN_SPEC.md). The ZIP contains React/Tailwind source but no screenshots; use the component code as the specification, and keep React/Gemini out of the shipped application.

The earlier implementation at [`3701bed`](https://github.com/scherenhaenden/photo-culler/commit/3701bedf924bc3a3927166ef3a86c59d688d1ab3) remains useful for legacy labels, behavior and migration checks. It does not override the Pro package's screen composition. In particular, the Pro Library is a grid with a separate primary-navigation rail and filter rail; it is not a preview/filmstrip/inspector hybrid. Cinema is a separate screen with an inspector.

## Sprint 01 gate: the Library screen

| Surface | Layout contract | First comparison |
|---|---|---|
| Native Slint | 46 px top header; 220 px primary navigation; 230 px Library filters; flexible toolbar and photo grid. | Capture at 1440 × 900 logical pixels, with the same fixture, selected photo, locale and scale as the prototype. |
| Slint WebAssembly | Same Library layout and controls in the browser; preserve the app bootstrap diagnostic outside the Slint canvas. | Capture at 1440 × 900 and a constrained width; check grid density and whether the filter rail follows the reference. |

The Library view must not contain the Cinema inspector. It needs the prototype's top navigation/actions, primary sidebar, Library-only filter rail, 46 px grid toolbar and 4:3 cards with cover crops, decision/rating metadata and selection treatment. Keep counts and state labels tied to Rust data, not hard-coded to the screenshot.

## Remaining design milestones

| ID | Milestone | Work | Acceptance |
|---|---|---|---|
| UX01 | Pin and capture the reference | Use the zip SHA from the design spec. Run the prototype only as a visual reference; capture its default Library state and selected-photo state at matched viewports with deterministic fixtures. | Reference and candidate captures identify source SHA/hash, viewport, scale, locale and fixture IDs. |
| UX02 | Port the shared shell | Port the header tabs/actions, 220 px navigation, decision counts, operator footer and shared tokens into Slint. Keep labels, active state, focus and spacing aligned. | Side-by-side shell geometry and state colors match at native and web sizes. |
| UX03 | Port Library filters and grid | Add the 230 px filter rail and grid toolbar. Implement the responsive 4:3 card grid, hover overlay, decision marker, rating, RAW badge, score and selection outline. | Same columns, card density, image crop, hover/selection states and filter affordances as the package. |
| UX04 | Connect the live Sprint 01 controller | Feed the Library from the shared typed Rust REST client in native and WASM. Keep unavailable write actions visible in their original positions and clearly disabled in the read-only demo. | Both surfaces show the same API-loaded image and metadata. Loading, empty, error and retry preserve the shell. |
| UX05 | Port Cinema and Compare | Implement the package's separate Cinema viewer/inspector and synchronized 4-up comparison. | Each screen uses its own reference layout and supports its defined selection/navigation behavior. |
| UX06 | Port Bursts, Timeline, Dashboard and dialogs | Port the remaining navigation destinations and import/export/settings/shortcuts surfaces, then connect operations as their Rust endpoints exist. | Layout matches the source; unavailable operations are explicit and recoverable. |
| UX07 | Verify full design parity | Capture each view on native and web and record geometry, typography, cropping, focus, responsive behavior and platform differences. | Each screen has a reviewed pair and documented resolution or approval for every material difference. |

UX01–UX04 are the Sprint 01 design gate. UX05–UX07 belong to later migration stages and are not implied by a passing one-photo demo. See [Sprint 01 remaining work](SPRINT_01_REMAINING.md) and the [64-task Rust/Slint roadmap](../RUST_SLINT_ROADMAP.md#ensamblaje-y-entrega).
