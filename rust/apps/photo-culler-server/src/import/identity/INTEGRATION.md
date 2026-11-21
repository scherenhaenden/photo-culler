# Import identity status

The filesystem scanner uses `compute_quick_hash` and `compute_full_hash` when
it observes completed manifest pages. The SQLite repository stores normalized
hash text and uses unique full-hash/size matches to preserve photo identity
across an unambiguous move. Existing duplicates remain separate catalog files
and are not guessed into a move.

The quick hash is SHA-256 over the decimal file size followed by the first
65,536 bytes. Full identity is SHA-256 over every file byte. Repository
reconciliation is performed after the full scan. Files outside the proven scan
coverage are excluded from both missing classification and move matching. When
a photo moves, its path-derived ID is retained at the new path. If the old path
is later reused by different contents, the new photo receives a versioned
deterministic ID derived from its path and full content hash; the prior ID
remains reserved with its original decisions.

Limited runtime evidence now exists for one unambiguous move, an identical copy
at another path, reuse of the moved-from path with different content, an
excluded partial scan, restart persistence and a repeated full scan on a small
synthetic catalogue. See
`docs/runtime-evidence/2026-11-21-import-reconciliation-smoke.md` and the exact
Rust manifest recorded there. Same-path modification, ambiguous duplicate
moves, interrupted scans, cross-platform paths and real-catalog behavior remain
unaccepted; do not generalize the synthetic result to those cases.
