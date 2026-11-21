# Import pairing status

`associate` groups source-relative media and sidecar paths without creating or
changing catalog photo identities. It keeps matching stems from separate
directories distinct, marks duplicate same-kind media as ambiguous, and leaves
sidecars unattached when their media group is ambiguous.

The module is registered in `import::pairing`, but the current SQLite
`finish_job` path resolves groups using its own `logical_key` and SQL checks;
this function is not called by that finalizer. The two implementations must be
kept behaviorally aligned or consolidated before declaring pairing complete.
In particular, extension support and sidecar attachment rules need to remain
consistent across discovery, pairing, and catalog finalization.

Runtime import acceptance is still pending. Record the candidate commit SHA and
verify RAW/JPEG association, sidecars, same-stem files in different folders,
duplicate candidates, and unchanged/moved photo identities using the procedure
in `docs/rust-slint-demo/RUN.md`.
