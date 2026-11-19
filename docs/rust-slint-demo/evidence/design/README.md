# S05 design capture evidence

The paired images below use the same viewport, fixture JPEG, and card layout.
The supplied `photo_culler_pro.zip` contains the React source, but no reference
screenshots, local photo files, lockfile, or installed dependencies. The PNG
reference renders below were recorded in an earlier capture as renders of that
source, with remote photo URLs replaced in a temporary extracted copy by the
bundled demo JPEG. I could not reproduce that render during the current review
because Node/npm and the source dependencies are unavailable here. Treat these
PNGs as historical source-derived evidence, not a freshly reproduced capture;
the ZIP and app source were not modified.

| Viewport | Reference | S05 base | With PR #155 icons |
|---|---|---|---|
| 1440 × 900 | [Reference](reference/pro-library-1440x900.png) | [Candidate](candidate/pro-library-1440x900.png) | [Icon overlay](icon-parity/pro-library-1440x900.png) |
| 1100 × 700 | [Reference](reference/pro-library-1100x700.png) | [Candidate](candidate/pro-library-1100x700.png) | [Icon overlay](icon-parity/pro-library-1100x700.png) |

## Current integrated S05 captures

The latest integrated candidate, head `59cb9f01746b4f66f180f8c9c2927489a8f7e5e7`, passed hosted run [#44563618333](https://github.com/scherenhaenden/photo-culler/actions/runs/44563618333) with all six jobs green. Its browser artifact contains fresh ten-card Slint/WASM Library screenshots at both design viewports, captured from the Rust Axum origin:

| Viewport | Screenshot SHA-256 | Fixture JPEG SHA-256 |
|---|---|---|
| 1440 × 900 | `20483b0f82004ffcebdcf332323264bbd591324c25a0bfc4bd896c3a0db7065d` | `cc20c24844d2fc6fca729fcb66cfe661960f7b56be41a3dd462a0f1b22cfb4f2` |
| 1100 × 700 | `240cc1bc576a71f4a737b8e2e1ce51873ccf3af546d74aaa505003b97e827d01` | `cc20c24844d2fc6fca729fcb66cfe661960f7b56be41a3dd462a0f1b22cfb4f2` |

The screenshots, current manifests and 20 state captures are in artifact
`rust-slint-demo-browser-acceptance-44563618333-1` (archive SHA-256
`4533c63f6935aecdcf6acb7a74b536c1d4edfd85b8f60ee23083c32a6c3ca3cb`). Open
the linked Actions run and its Artifacts section to inspect them.

## Native acceptance screenshots

The integrated hosted native acceptance run [#44563618333](https://github.com/scherenhaenden/photo-culler/actions/runs/44563618333)
passed all six CI jobs on head `59cb9f01746b4f66f180f8c9c2927489a8f7e5e7`.
The PNGs listed below are preserved captures from earlier run
[#44546749693](https://github.com/scherenhaenden/photo-culler/actions/runs/44546749693)
and are retained as historical evidence. The latest native captures are in
the current run's platform artifacts; they verify the same ten-card fixture,
while the separate live smoke verifies the one-photo REST gallery. Native
acceptance also verifies that the owned server exits and releases its port
after close, while an external server remains healthy.

| Platform | Current `gallery-smoke.png` | Dimensions | SHA-256 | Native GUI artifact SHA-256 |
|---|---|---:|---|---|
| Linux x86-64 | Run #44563618333 | 1439 × 880 | `d193b2a17eb9d0667fe121928eaaacdc2b255c68f63d06f77cf31fd57d93224d` | `90ff19b24b6379950749a0ac7779144dc4cade75735bc0b89219f30d31492693` |
| macOS arm64 | Run #44563618333 | 1212 × 840 | `3aff5ce1cc4690a5febf2dfcd53c54b82ef76928fe75e4356975a8c4101225b0` | `0cd28a1c7dc31aaace2d3ebc7453163a113c2463851ab154b4c8a0c1a0ff8d51` |
| Windows x86-64 | Run #44563618333 | 1442 × 932 | `22ce0f81c5bbc9e36db2d6e876213966521b706cd6fbf0e4c588f97fbae8779f` | `cbe97bd4a13b2e31bb9ea3f8457e6551120d9cf642395a7b7b70d5f8cf07d4ff` |

The historical local PNGs below remain useful for comparing earlier revisions;
they are not the current run's screenshots.

| Platform | Native capture | Dimensions | SHA-256 |
|---|---|---:|---|
| Linux x86-64 | [Window](native/linux-x86_64-445467.png) | 1439 × 880 | `8ddf029db66049b8fa3d338c317869c833c38abd9612e6a87c302695a9aa9996` |
| macOS arm64 | [Window](native/macos-arm64-445467.png) | 1212 × 840 | `dfb10ff595c3b75c5fbd3f93cce8e5b398b57f4310ccd5e2000e2d322fe1832f` |
| Windows x86-64 | [Window](native/windows-x86_64-445467.png) | 1442 × 932 | `1b563e43abc74cbc52c709f89072131e2e73f6cc2383324de7f2aa27a489173e` |

[`native/manifest.json`](native/manifest.json) records the platform artifact
names and GitHub SHA-256 digests. Windows uses Slint's software backend in the
hosted runner; normal packaged launches retain their standard renderer choice.

The candidate comes from commit `afe088242bdcf173eb8e83c9571bf598f52c4e86`.
It uses the explicit browser-only smoke selector and ten records with synthetic
metadata; the normal REST view does not invent those fields. The manifest records
the archive, fixture, viewport, backing canvas, and screenshot hashes.

The historical base screenshots show text icon placeholders. PR #155 adds Slint
vector paths; those paths are part of the current integrated S05 candidate and
the latest browser/native capture stack.

## Paired-image review

The main composition is aligned: 46 px header and toolbar, 220 px navigation,
230 px filter rail, 4:3 cards, 12 px grid gaps, and the same 5×2 / 4×3 responsive
column counts. The card crop, selected outline, decision badges, and overall dark
palette are close at both viewports.

The paired reference/candidate PNGs above are still from an earlier source
render. The latest code now implements the watermark, camera order,
ISO/sharpness slider details, 1100 px header behavior, and vector icon paths.
The current code has fresh browser and native captures with automated canvas,
fixture-image, content and state checks. The reference ZIP contains no
authoritative screenshots, so those passes do not establish exact pixel parity
against an original raster; compare the latest artifact against the source
package during design review.

Known design gaps before claiming exact pixel parity:

- The operator footer is a circular photo in the source design; the app uses a
  person-outline fallback because the ZIP has no local avatar.
- The paired local reference render is historical, and the ZIP provides no
  original reference raster for a measured pixel-difference comparison.
- The selected 10-record fixture repeats one JPEG and has synthetic metadata;
  only the normal single-photo REST route represents live data.

The paired S05 reference/candidate images at the top are historical browser/WASM
captures; the current browser and native screenshot artifacts are recorded
above. S07 passed on Linux, macOS and Windows in run #44563618333. The icon
paths from PR #155 are included in the tested stack. Scope remains the initial
Library screen; Cinema, Compare, Bursts, Timeline, dashboard and remaining
product dialogs are later design work.
