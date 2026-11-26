# Linux selection acceptance for Sprint 03

This acceptance run exercises the integrated Sprint 03 desktop package against a temporary SQLite catalog and a 13-photo JPEG fixture. It is designed for the Linux Xvfb runner and never targets the developer's active display.

The sequence is: prepare the fixture and package, seed a disposable catalog, start the staged desktop under Xvfb/Openbox, drive selection and decision actions, verify pages and counts against SQLite, close and reopen the package, then verify stable IDs and unchanged JPEG hashes.

The workflow should invoke this after the feature branches for keyboard actions and global gallery counts have been integrated into the sprint branch. The current Sprint 03 branch does not yet contain those controls, so this run cannot pass against its current head.

## Acceptance assertions

- `photo-01` and `photo-02` are visible on the first 12-photo page. `photo-13` is visible on the next page.
- KEEP on `photo-01`, REJECT on `photo-02`, KEEP on `photo-13`, then correcting `photo-01` to REJECT yields KEEP=1, REJECT=2, UNRATED=10, total=13.
- The KEEP filter returns only `photo-13` regardless of which page was open when the filter changed.
- The same decisions, IDs, counts and KEEP result are present after normal close and reopen.
- All 13 source JPEG hashes match the fixture manifest after the run.
- Artifacts include screenshots of both pages and the filtered page, the desktop log, database snapshots, and a JSON run manifest with the tested source SHA and binary hashes.

The acceptance must seed stable photo IDs in lexicographic order because the catalog page cursor is ordered by `photo_id`, not the displayed filename. `photo-01` through `photo-13` are therefore also the IDs in the fixture catalog.
