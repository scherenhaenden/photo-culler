# Third-party notices and distribution status

This is a source-based inventory for the repository at commit `ad1bb99` (18
November 2026). It records license declarations and unresolved distribution
checks; it is not a generated notice bundle for a release artifact. Rebuild the
inventory from the exact lockfiles, features, and native libraries used for
each artifact before publishing binaries.

## Project license

The repository README states MIT, and `rust/Cargo.toml` sets the workspace
package license to `MIT`. This commit adds the corresponding MIT text in
[`../LICENSE`](../LICENSE). That declaration covers Photo Culler-authored code
only; it does not relicense dependencies. The repository does not contain a
copyright assignment or contributor-rights record. The notice uses the
collective `Photo Culler contributors` attribution; maintainers should confirm
it represents the relevant copyright holders before making a distribution.

## Rust image stack present in the lockfile

The `photo-culler-image-engine` crate enables only `image`'s `jpeg`, `png`, and
`tiff` features (`rust/Cargo.toml`). The current lockfile resolves `image`
0.25.10, `png` 0.18.1, `tiff` 0.11.3, and `zune-jpeg` 0.5.15. These are source
dependencies; this commit does not establish that an application containing
them has been built or distributed.

| Component | Evidence and license status | Distribution action |
| --- | --- | --- |
| `image` | Upstream project identifies MIT OR Apache-2.0. | Choose and include one permitted license text with notices when this crate is shipped. |
| `png` | Upstream project identifies MIT OR Apache-2.0. | Include the selected license text and copyright notices when shipped. |
| `tiff` | The upstream `image-rs/image-tiff` project identifies MIT. Verify the resolved crate archive's license files before release. | Include its MIT notice and license text when shipped. |
| `zune-jpeg` | Present transitively in the lockfile; its exact 0.5.15 archive metadata and notices were not available in this checkout. Upstream package license options must be checked against that exact version. | Do not treat this inventory as a complete JPEG notice bundle; inspect the resolved archive and include all required notices. |

Authoritative upstream references: [`image`](https://github.com/image-rs/image),
[`png`](https://github.com/image-rs/image-png),
[`tiff`](https://github.com/image-rs/image-tiff), and
[`zune-image`](https://github.com/etemesi254/zune-image). Version evidence is
from `rust/Cargo.lock`; the version-specific license text/metadata was not
downloaded for every crate here.

## Slint and browser distribution

There is no Slint dependency, generated Slint source, or Slint artifact in the
workspace at this commit. The roadmap describes a future Slint desktop/WASM
target, so no Slint license is selected and no Slint distribution claim is
made here. Before adding it, choose a license plan for the exact Slint version
and product use. Slint's official [FAQ](https://slint.dev/faqs) describes
GPLv3, a royalty-free license, and a commercial license; the exact current
[terms](https://slint.dev/terms-and-conditions) determine the obligations. The
royalty-free option includes attribution obligations. Record the selected
license and preserve its notices in both native and web distributions.

## RAW decoding and other native codecs

The current Python package declares `rawpy` in `pyproject.toml`, but the
resolved Python distribution, its bundled/shared LibRaw arrangement, and the
license notices in the built application have not been inspected as an
artifact. LibRaw's official [licensing page](https://www.libraw.org/about)
offers LGPL-2.1 and CDDL-1.0 licensing and identifies additional inherited
code. Do not assume the Python wrapper's license settles the terms of a binary
that includes or links LibRaw. Determine whether LibRaw is bundled, dynamically
linked, or provided by the host on every supported platform, then include the
corresponding source/license and notices required for that mode.

The Rust `image` feature list does not include a RAW codec. HEIC/HEIF support
appears in the scanner's extension classification but no decoder is declared
in the current Rust image-engine manifest. Extension recognition is not codec
support. No distribution or patent-clearance conclusion is made for RAW,
HEIF/HEIC, JPEG, or other format patents.

## Release gate

Before calling a release distributable, generate a per-artifact inventory from
its locked dependency graph and bundled libraries; verify each exact package's
license expression and license files; include the chosen license text and
copyright notices; and inspect the final archive/installer for the expected
notices. In particular, resolve the exact `zune-jpeg` and `rawpy`/LibRaw
notices, and the Slint license after that dependency is introduced.

Checked against upstream pages on 2026-11-18. These pages can change; release
reviews must recheck the license files shipped with the exact resolved source
versions.
