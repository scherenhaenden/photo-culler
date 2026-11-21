# Session authentication runtime smoke

**Date:** 2026-11-21 (UTC)
**Candidate:** uncommitted worktree based on `26991eb`
**Host:** Darwin arm64; server bound to loopback only
**Data:** isolated under `/private/tmp/photo-culler-auth-e2e`; no real photo library or credentials printed in this record.

## Server and client flows

The server smoke exercised health, protected API access, pairing, a second
client, revocation, and startup behavior:

- Health returned 200. An unauthenticated catalog read returned 401. Unknown
  API and non-API paths returned 404.
- A one-use pairing code returned an Editor bearer; the bearer read protected
  gallery data successfully. The first client requested another one-use code,
  paired a second client, and both could read the catalog.
- Revoking client one's session returned 204 and its next API request returned
  401. Client two remained valid. A non-interactive server withheld its initial
  pairing code.

The CLI smoke then used `auth pair`, `auth pairing-code`, and `auth logout`:

- Two clients paired independently using codes read through stdin. The
  credential file was mode `0600`; no token or pairing code was captured in
  this report.
- Both clients could read protected galleries. After the first client logged
  out, its local credential file was removed and its bearer received 401; the
  second client remained valid and logged out separately.
- An unpaired catalog read returned 401.

The post-review CLI changes additionally reject symlink following when opening
credential files on Unix and remove the local file even when remote logout
cannot be confirmed. Those error paths have not been exercised in this smoke.

## Scope

This is limited loopback auth/API evidence, not visual native/browser
acceptance, service-managed pairing, HTTPS/LAN deployment, OS-keychain storage,
role provisioning, session refresh, or cross-platform acceptance. The server
currently rejects non-loopback binds. Native and browser clients compiled, but
their session screens were not accepted in a running UI. No test suite was
run. Temporary token files, runtime data, and build targets were removed after
the checks.

## Provenance limitation

The contemporaneous capture retained the base commit but not a non-Markdown Rust input manifest, executable SHA-256, macOS version, or Rust/Cargo versions. The runtime observations above therefore remain useful as a historical smoke record but cannot be attributed to an exact current candidate snapshot. Later CLI credential hardening must not be inferred as covered by this run.
