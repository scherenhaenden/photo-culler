# S05 design capture evidence

The paired images below use the same viewport, fixture JPEG, and card layout.
The reference was rendered from the supplied `photo_culler_pro.zip` source. In a
temporary extracted copy, only the remote photo URLs were changed to the bundled
demo JPEG so the comparison is deterministic; the ZIP and application source
were not modified.

| Viewport | Reference | Slint candidate |
|---|---|---|
| 1440 × 900 | [Reference](reference/pro-library-1440x900.png) | [Candidate](candidate/pro-library-1440x900.png) |
| 1100 × 700 | [Reference](reference/pro-library-1100x700.png) | [Candidate](candidate/pro-library-1100x700.png) |

The candidate comes from commit `afe088242bdcf173eb8e83c9571bf598f52c4e86`.
It uses the explicit browser-only smoke selector and ten records with synthetic
metadata; the normal REST view does not invent those fields. The manifest records
the archive, fixture, viewport, backing canvas, and screenshot hashes.

This is browser/WASM evidence, not a native-window capture. The current candidate
still needs the hosted native GUI acceptance in S07. Iconography is tracked by
the separate icon-parity PR #155, and the S05 candidate captures shown here do
not include that branch. The scope is the initial Library screen; Cinema, Compare,
Bursts, Timeline, dashboard, and the remaining product dialogs are later design
work.
