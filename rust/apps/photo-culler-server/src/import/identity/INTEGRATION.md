# Task 23 integration notes

The server crate, its root module declarations, and the common import test
harness do not exist at base `a68f360`. This folder therefore contains a
dependency-free Rust module with injected `Read`/`Seek` inputs and pure
reconciliation records, plus unit tests that can be run directly with `rustc`.

When the server crate is created, the integrator still needs to:

1. Register `import::identity` in the server's module tree and connect its
   filesystem scanner to `compute_quick_hash` and `compute_full_hash`.
2. Map `KnownFile` and `ObservedFile` to catalog rows and apply returned
   `Change` values in the task 24 worker's transaction. The move decision must
   only update catalog paths; hashing and reconciliation never write originals.
3. Add catalog-backed acceptance coverage for move, modification, and copy
   behavior once the shared repository test double is available.

The quick hash is compatible with the Python implementation: SHA-256 over the
decimal file size followed by the first 65,536 bytes. Full identity is SHA-256
over every file byte. A unique full-hash match may reuse identity after a move;
duplicate candidates remain separate additions and missing entries rather
than being guessed into a move.
