# Photo Culler Pro design reference

## Source and scope

The visual authority for the Slint port is the user-provided `photo_culler_pro.zip` design package. Its SHA-256 is `66b06c65a453182dc0bfcf24efa07460255d0a914b5a606b5fafeb990e8f59c7`. The package identifies itself as `PHOTO_CULLER_PRO`; `src/App.tsx` opens in the Library view. It contains React/Tailwind source, sample data and no screenshot PNGs.

Use the package to reproduce the product's layout, visual states, labels and interactions. Do not ship its React, Vite, Gemini or JavaScript runtime. The application UI and behavior remain Rust/Slint, with same-origin REST/WASM on the web target. The older egui and legacy web code at `3701bed` is useful for migration context; when its layout conflicts with this Photo Culler Pro package, this package is the design reference for the Pro screen.

## Initial Library screen

The reference opens to a full-window Library screen. Its layout is one 46 px top header above a three-column content row:

| Region | Reference geometry and content |
|---|---|
| Top header | 46 px high, `#050505`, 16 px horizontal padding. `CULLER / 01` brand; Library, Cinema, Compare (4-Up), Bursts and Dashboard tabs; 208 px global search; Import and Export; CPU, processing, shortcuts and settings controls. |
| Primary navigation | Fixed 220 px, `#050505`. Library, Compare (4-Up), Bursts Engine, Timeline, Cinema Specs and System Overview; selection counts; operator/session footer. |
| Library filters | Fixed 230 px, `#050505`, shown on the Library view. Header with matching-result count and Reset; Cull Decision, Camera Body, ISO Threshold, Min Sharpness, Min Star Rating and RAW Only controls. |
| Library content | Flexible width, `#050505`. A 46 px `#111111` toolbar contains catalogue count, sort/order, selected-item actions, Select All and Small/Med/Large grid density. A padded scrolling grid fills the rest. |

The Library view has **no right inspector panel**. The photo inspector belongs to Cinema. Do not turn the Library into a large preview, filmstrip and inspector composition.

At 1440 px viewport width, the reference uses the 220 px navigation rail, 230 px filter rail and flexible grid. Grid spacing is 12 px. Cards have a 4:3 ratio, 16 px corners, cover-cropped images, a 2 px orange selected ring, RAW and score badges, a decision marker, and a hover overlay with rating stars, filename, ISO and Inspect. The reference's Small density uses 2/3/4/6 columns at base/`sm`/`md`/`lg`; Medium uses 1/2/3/4/5 at base/`sm`/`md`/`lg`/`xl`; Large uses 1/2/2/3 at base/`sm`/`md`/`lg`. Preserve those responsive density choices in Slint rather than capping the default Medium grid at three columns.

The interaction states to preserve are selected and unselected cards, shift/meta/control multi-select, hover metadata, sort direction, grid density, selection batch actions, filter selection/reset and opening Cinema. The current Sprint 01 read-only backend may disable write actions, but keep their positions and make their unavailable state clear.

## Shared visual tokens

| Role | Reference value |
|---|---|
| Canvas / navigation / content | `#0F1012` canvas; `#050505` navigation and content; `#111111` surfaces |
| Text | `#E3E2E4` primary; white at 60% and 40% for secondary and muted text |
| Accent / decisions | `#FF6B35` selection and active state; `#25A18E` Keep; `#F7C59F` Alt; `#FF6B35` Reject |
| Typography | Inter/sans for interface text; monospace for technical values and labels; uppercase, tracked section labels |
| Geometry | 46 px header and library toolbar; 220 px nav; 230 px filter rail; 12 px grid gaps; 16/12/8 px rounded corners by hierarchy |

Desktop and WebAssembly should share this Slint view at the same logical viewport and use their platform adapters for input, window sizing and image loading. Keep the browser's accessible startup diagnostic outside the Slint canvas as the current bootstrap does.

## Reference component map

| Design package source | Slint ownership |
|---|---|
| `src/components/TopHeader.tsx` | Shared Slint app header and action callbacks |
| `src/components/SidebarNav.tsx` | Primary navigation and decision totals |
| `src/components/FilterSidebar.tsx` | Library-only filter rail |
| `src/components/LibraryGrid.tsx` | Library toolbar, responsive photo cards and selection states |
| `src/components/CinemaViewer.tsx` | Later Cinema view and its inspector; not the Library right rail |
| `src/components/CompareView.tsx`, `BurstsView.tsx`, `TimelineView.tsx`, `DashboardView.tsx` | Later view milestones; preserve their distinct layouts |

The prototype's sample photo URLs are not suitable runtime dependencies for a Rust desktop/WebAssembly build. Visual acceptance should use one deterministic local fixture set for both the React reference and the Slint candidate, or clearly identify the different image content while comparing geometry and controls.

## Port roadmap

| ID | Milestone | Work | Acceptance |
|---|---|---|---|
| UX01 | Pin the reference | Keep this hash and component map with the sprint record. Capture the prototype and candidate at matched 1440×900 and constrained widths with the same deterministic images, selection and locale. | Paired captures are reproducible and identify package hash, viewport, scale and fixture IDs. |
| UX02 | Port the shell | Match the 46 px top bar, tab states, global search/actions, 220 px primary navigation, counters and operator footer. | Geometry, labels, color roles and active/inactive states match the package. |
| UX03 | Port the Library screen | Add the separate 230 px filter rail and the library toolbar/card grid. Remove the preview/filmstrip/inspector hybrid from Library. | Matched captures show the same columns, card ratio/crop, selected ring, badges, hover overlay, counts and filter controls. |
| UX04 | Connect the live demo | Feed the ported screen from the shared Rust REST controller and same-origin WASM transport. Wire selection and available actions; clearly mark unavailable write actions. | The real Rust API thumbnail appears in the reference card layout; loading, empty, error and retry preserve the shell. |
| UX05 | Port Cinema and Compare | Implement the prototype's separate Cinema inspector and synchronized 4-up compare. | Each mode has its own matching layout and selection behavior. |
| UX06 | Port Bursts, Timeline, Dashboard and workflows | Port the remaining reference modes, import/export/settings/shortcuts affordances, and connect behavior as the Rust endpoints exist. | Layout and state transitions match the reference; unsupported actions state why they are unavailable. |
| UX07 | Verify product-wide parity | Capture and compare each mode on native and browser targets; record renderer differences and fixes. | Every reference screen has a reviewed pair and explicit sign-off. |

Sprint 01's design gate is UX01–UX04 for the entry Library screen. UX05–UX07 are later Rust migration milestones; do not claim full Photo Culler Pro parity from the one-photo Sprint demo.
