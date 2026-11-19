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

## Native acceptance screenshots

The rebased hosted native acceptance run [#44550496763](https://github.com/scherenhaenden/photo-culler/actions/runs/44550496763)
passed all six CI jobs on head `06b76fccf15d96e9bd0e14290c0be2f7a82afa41`.
The PNGs listed below are preserved captures from the earlier run
[#44546749693](https://github.com/scherenhaenden/photo-culler/actions/runs/44546749693),
not captures from #445504. Both runs use the explicit ten-card fixture, which
repeats the bundled JPEG and uses synthetic metadata; the separate live smoke
verifies the one-photo REST gallery. Native acceptance also verifies that the
owned server exits and releases its port after close, while an external server
remains healthy.

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

The base screenshots show text icon placeholders. PR #155 adds Slint vector
paths on top of the design branch; the icon overlay was separately captured and
visually checked at both sizes on the same fixture. See
[`icon-parity`](icon-parity/capture-manifest.json) for those PNGs and hashes.
That branch is ready for review but remains a separate PR, so its icons are not
in the S05 base captures above.

## Paired-image review

The main composition is aligned: 46 px header and toolbar, 220 px navigation,
230 px filter rail, 4:3 cards, 12 px grid gaps, and the same 5×2 / 4×3 responsive
column counts. The card crop, selected outline, decision badges, and overall dark
palette are close at both viewports.

The captures are from earlier code and do not include the latest local S05
adjustments. The current worktree now implements the watermark, camera order,
ISO/sharpness slider details, and 1100 px header wrapping/visibility. Those
changes pass the source-level parity checks and compile for native and WASM, but
need fresh browser/native captures before they count as visually verified.

Known design gaps before claiming exact pixel parity:

- Header/sidebar icons are text stand-ins in the S05 base captures. Vector
  replacements render correctly in the separately captured PR #155 overlay but
  are not in the #154 base or the native acceptance stack yet.
- The reference operator footer is a circular photo; the candidate uses a
  person-outline fallback because the source only points to an unavailable
  remote avatar and supplies no local image.
- The captured #154 candidate predates the latest local S05 adjustments. A fresh
  1100 px capture must confirm the full toolbar, `/01` logo wrap, and menu
  visibility after those changes.
- The selected 10-record fixture repeats one JPEG and has synthetic metadata;
  only the normal single-photo REST route represents live data.

The paired S05 reference/candidate images at the top are browser/WASM captures;
the native-window captures are recorded separately above. S07 passed on Linux,
macOS and Windows in run #44550496763. The icon overlay belongs to the separate
PR #155 and is not in the #154 base. Scope remains the initial Library screen;
Cinema, Compare, Bursts, Timeline, dashboard and remaining product dialogs are
later design work.
