# Slint library gallery shell — Sprint 01 S05

This UI groundwork ports the first library view from the reference at 3701bed while retaining platform composition differences:

- Native: 220 px archive sidebar, compact photo list, contained selected-photo preview, 260 px inspector and status footer. The reference desktop window minimum is 1100 × 700 logical pixels.
- Web: dark archive chrome, responsive gallery cards (200 px minimum, 180 px cover thumbnails, 20 px gaps), and a 320 px inspector once the content is wide enough. Narrow layouts drop the inspector to keep the grid usable.
- Decision colors are semantic: desktop uses red rejection while the web reference uses orange; Keep uses teal on desktop and blue on web. Best, Alternate and Unrated retain their own reference colors.

gallery.slint exports GalleryPhoto and LibraryGallery. demo_window.slint keeps the existing runtime properties and callbacks (view-state, photo-name, error-message, photo-thumbnail, photo-index, photo-count, navigation-label, retry, previous-photo, and next-photo) and adds these inputs for the real gallery shell:

- gallery-photos: [GalleryPhoto] supplies the current page and thumbnail images.
- selected-photo-id, gallery-title, service-status and web-layout select visible state, archive label and platform composition.
- kept-count, alternate-count, rejected-count and unrated-count feed the archive statistics.
- select-photo(id) and select-archive(id) return list and archive actions.

When gallery-photos is empty, the existing photo-name and photo-thumbnail properties render a single-photo fallback; the host must still supply the thumbnail image. Loading, empty and connection-error states keep the archive shell visible; the error panel invokes retry. Selection and refresh callbacks must be wired by the host. Search and decision controls are displayed disabled until their state/action contract is integrated. Dashboard is shown as unavailable instead of pretending to switch screens.

The root crate currently compiles DemoScaffold from demo_scaffold.slint. DemoWindow is the primary screen; demo_window.slint also exports a DemoScaffold compatibility alias, so the existing desktop/browser Rust adapters keep their generated type when the build entry changes. The integrator must select demo_window.slint, add it and library/gallery.slint to build rerun tracking, and wire the added fields/callbacks before this becomes the live entry screen. This PR deliberately does not modify Rust entry points, manifests, or workspace registration.

## Checks

Run source-level UI contracts:

    python3 rust/apps/photo-culler-slint/ui/library/test_gallery_parity.py
    python3 rust/apps/photo-culler-slint/ui/components/test_components.py

Compile the actual DemoWindow source with the locked Slint compiler without changing root registration:

    PHOTO_CULLER_SLINT_ENTRY=ui/demo_window.slint cargo check --manifest-path rust/Cargo.toml -p photo-culler-slint --lib --locked

This checks Slint parsing/type-checking and generated library code. The window binary check remains an integration gate until the Rust root selects DemoWindow.
