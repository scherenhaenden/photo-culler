# S05 design capture evidence

The paired images below use the same viewport, fixture JPEG, and card layout.
The reference was rendered from the supplied `photo_culler_pro.zip` source. In a
temporary extracted copy, only the remote photo URLs were changed to the bundled
demo JPEG so the comparison is deterministic; the ZIP and application source
were not modified.

| Viewport | Reference | S05 base | With PR #155 icons |
|---|---|---|---|
| 1440 × 900 | [Reference](reference/pro-library-1440x900.png) | [Candidate](candidate/pro-library-1440x900.png) | [Icon overlay](icon-parity/pro-library-1440x900.png) |
| 1100 × 700 | [Reference](reference/pro-library-1100x700.png) | [Candidate](candidate/pro-library-1100x700.png) | [Icon overlay](icon-parity/pro-library-1100x700.png) |

## Native acceptance screenshots

The final hosted native acceptance run [#44546749693](https://github.com/scherenhaenden/photo-culler/actions/runs/44546749693)
passed all six CI jobs on commit `68959bf702c554839b720d80fd51bfbad6824317`.
The screenshots below show the current Library window with the explicit ten-card
fixture; each card repeats the bundled JPEG and uses synthetic metadata. The
separate live smoke still verifies the one-photo REST gallery. Each native run
also verified the owned server exits and releases its port after close, and an
external server remains healthy.

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

The captures still show visible differences to resolve before claiming exact
pixel parity:

- Header/sidebar icons are text stand-ins in the S05 base captures. Vector
  replacements render correctly in the separately captured PR #155 overlay but
  are not in the #154 base or the native acceptance stack yet.
- The faint “CUL” watermark behind the reference's empty lower grid is absent.
- At 1100 px the reference keeps BURSTS and DASHBOARD in the header while the
  Slint capture drops them; the reference logo wraps `/01` and Slint keeps it in
  one line. Both captures clip far-right header actions.
- The camera filter order differs: reference NIKON Z9, Sony A7R IV, Canon EOS R5;
  candidate Canon EOS R5, NIKON Z9, Sony A7R IV.
- ISO shows `≤ 12800` and a clear right-end thumb in the reference; the candidate
  shows `12800` without a visible thumb. The reference's teal sharpness thumb at
  0% is also barely visible in Slint.
- At 1100 px the smoke banner and READ ONLY label make the candidate toolbar clip
  Select All and most density choices; the live single-photo layout should be
  checked at that width separately.
- The reference operator footer is a circular photo; the candidate uses `OP`
  initials. Decision actions are disabled in the candidate by the read-only
  Sprint 01 contract.
- The selected 10-record fixture repeats one JPEG and has synthetic metadata;
  only the normal single-photo REST route represents live data.

The paired S05 reference/candidate images at the top are browser/WASM captures;
the native-window captures are recorded separately above. S07 passed on Linux,
macOS and Windows in run #44546749693. The icon overlay belongs to the separate
PR #155 and is not in the #154 base. Scope remains the initial Library screen;
Cinema, Compare, Bursts, Timeline, dashboard and remaining product dialogs are
later design work.
