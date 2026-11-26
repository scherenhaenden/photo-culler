# Linux selection acceptance for Sprint 03

This acceptance run exercises the integrated Sprint 03 desktop package against a temporary SQLite catalog and a 13-photo JPEG fixture. It is designed for the Linux Xvfb runner and never targets the developer's active display. The input driver uses OCR for controls and `xdotool` for deterministic card coordinates because the current gallery cards do not show filename labels, including on hover. A failure retains the screenshots and OCR boxes for diagnosis.

The sequence is: prepare the fixture and package, seed a disposable catalog, start the staged desktop under Xvfb/Openbox, select cards and apply KEEP/REJECT, verify the visible KEEP filter and reach the thirteenth photo through the UI's next-photo control, close and reopen the package, then verify persistent decisions, IDs, counts and unchanged JPEG hashes. SQLite snapshots and OCR output are retained alongside screenshots.

The workflow runs this after building the feature branch package. It requires the selection controls and global decision summary integrated from Sprint 03's counts/UI feature.

## Acceptance assertions

- `photo-01` and `photo-02` are visible on the first 12-photo API page. The deterministic cursor order puts `photo-07` on page two.
- KEEP on `photo-01` and REJECT on `photo-02` yield KEEP=1, REJECT=1, UNRATED=11, total=13.
- The KEEP filter shows `photo-01` and omits the REJECT photo.
- The same decisions, IDs, counts and KEEP result are present after normal close and reopen.
- All 13 source JPEG hashes match the fixture manifest after the run.
- Artifacts include screenshots, OCR box JSON, desktop logs, database snapshots, and a JSON run manifest with the tested source SHA, IDs and source JPEG hashes.

Imported photo IDs are content-derived, so the API cursor's `photo_id` order differs from filenames. The desktop sorts each loaded page by filename descending because the fixture images have no EXIF capture time. The runner reads the first API page from SQLite, mirrors that display sort, and targets cards by their grid position. The second-page assertion derives its expected filename from the catalog cursor order, then checks that the visible UI reaches it.
