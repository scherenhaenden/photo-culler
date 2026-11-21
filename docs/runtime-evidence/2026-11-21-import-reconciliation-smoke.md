# Import identity reconciliation runtime smoke

**Date:** 2026-11-21 (UTC)
**Candidate:** uncommitted worktree based on `26991eb`
**Rust input manifest after fix:** 164 non-Markdown files, SHA-256 `a8ab87a492ad525d1f8dc3ed56e84d1b7e9aa4ccce98533415d366b91c7b4e73`
**Host:** Darwin arm64; server bound only to `127.0.0.1:18767`
**Data/source:** isolated under `/private/tmp/photo-culler-import-reconcile-20261121-a5f3`; synthetic fixture only.

## Fixture

The initial file `source/frame.jpg` had SHA-256 `f996f797fde19c10187d19538ad738a18235c6a9b03d34b85b896cff61401922`. A second JPEG with different content had SHA-256 `cc20c24844d2fc6fca729fcb66cfe661960f7b56be41a3dd462a0f1b22cfb4f2`. Both were copied from the repository's synthetic fixture; the source fixture was not modified.

## Reconciliation flow

1. Initial estimate found one JPEG; import completed with one new logical photo, ID `8fc019622595a08b02c12354a642143a`. A decision write set it to `keep`, revision 1.
2. Moved `frame.jpg` to `moved/frame.jpg` without changing bytes. The first reimport failed and rolled back with `import contains conflicting move identities; no catalog changes were applied`. The server log confirmed the reason. The existing photo and its `keep` decision remained intact.
3. Root cause: `assign_finish_groups` counted a unique moved candidate as a conflict when there was no exact-path ID (`exact_photo_id IS NULL`). The CASE condition in `rust/apps/photo-culler-server/src/import/repository.rs` now counts a conflict only when an exact-path ID exists and the move candidate differs. The separate guard for multiple ambiguous move candidates remains in place.
4. Repeated the same import after rebuilding. It completed; scan revision reported `moved_files=1`, `new_files=0`, `missing_files=0`. The photo retained ID `8fc019622595a08b02c12354a642143a` and decision `keep`, revision 1.
5. Copied the moved original to `duplicate.jpg` and copied the different JPEG into the now-reused `frame.jpg` path. Import completed with two new records and no missing/modified records. The duplicate received ID `037cfabd092eeb5c916b4dc97dd1c717`, and the reused `frame.jpg` received `0551ecd4fd092c7e66ad9a8c6bb2f97b`; both were `undecided`, revision 0. The original moved photo retained its old ID and `keep` decision. The two byte-identical files therefore remained distinct photo records.
6. Ran a partial scan with `exclude_patterns=["moved/**"]`. Estimate found two in-scope files; import completed with `missing_files=0`. The out-of-scope moved photo remained visible with its prior decision.
7. Restarted the server with the same data directory. All three IDs and decisions remained. A full scan after restart completed with `new_files=0`, `modified_files=0`, `moved_files=0`, `missing_files=0`.

The initial failure is retained in this record because it exposed the SQL defect. The corrected candidate was rebuilt and the same move scenario passed before path reuse, duplicate and partial-coverage checks.

## Scope

This is limited runtime evidence for a small synthetic catalog and the tested move, duplicate, reused-path, exclude and restart cases. It does not accept large catalogs, RAW/JPEG pairing, interrupted-job recovery, real user libraries, concurrent scans, UI, NAS, authentication, or cross-platform behavior. No test suite was run. The temporary server target and fixture/database were removed after capture.
