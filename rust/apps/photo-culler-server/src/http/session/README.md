# Session authentication and operation authorization

## Current candidate

The persistent routers install `http::session::runtime::protect` after all API
and asset routes are merged. `OriginPolicy` remains a separate outer ingress
check. API reads and writes require a bearer except for the explicit public
routes below; unknown methods and paths fail closed. Static files are public
only when their exact paths appear in the validated `AssetBundle` manifest.

`SessionAuth` is an in-memory store. It generates 256-bit secrets with the OS
random source, stores only SHA-256 verifiers, prunes expired entries, and caps
the store at 256 sessions and 32 outstanding pairing codes. Editor sessions
expire after 12 hours; one-use pairing codes expire after 5 minutes. Restarting
the server invalidates all sessions and codes.

Startup behavior:

- `--owned` provisions an Editor session and returns its token only in the
  private JSON ready line consumed by the desktop launcher.
- A manually started server provisions one pairing code. It is printed to
  stderr only when stderr is an interactive terminal. When started under a
  service manager or another non-interactive process, no initial pairing code
  is exposed; there is not yet a protected-file or service pairing handoff.

## Session endpoints

| Method and route | Access | Result |
| --- | --- | --- |
| `POST /api/v1/session/pair` | Public route plus same-origin ingress checks | Redeems a one-use code and returns an Editor bearer with `Cache-Control: no-store`. |
| `POST /api/v1/session/pairing-codes` | Editor bearer | Returns one new short-lived pairing code for another client, `no-store`. |
| `DELETE /api/v1/session` | Caller’s Editor bearer | Revokes only that session and returns 204. |

The pairing route requires an exact loopback `Origin` through the separate
ingress policy. The server rejects non-loopback bind addresses in both argument
parsing and lifecycle startup; remote/LAN use therefore requires a separately
reviewed HTTPS deployment design. Codes and bearer tokens are never accepted
in URLs. Error responses do not echo credentials or internal store details.

## Protected route classes

- **Public:** `GET`/`HEAD /api/v1/health`, `POST /api/v1/session/pair`, and
  `GET`/`HEAD` of the validated static asset paths.
- **Read:** registered catalog GET/HEAD routes; recipe GET; preview GET;
  analysis job GET; source, import job, and scan revision GET routes.
- **Edit:** decision PUT; recipe PUT and undo/redo; import estimate/start,
  rescan and job controls; analysis submit and job controls; pairing-code issue
  and self-revoke.
- **AdministerNas:** no NAS HTTP routes are currently mounted. The pure role
  policy supports this operation, but there is no administrator issuance flow.
- All other API methods and paths are denied. Adding a route requires adding
  its operation classification explicitly.

Pairing currently creates Editor sessions. Reader and NAS Administrator
provisioning, NAS routes, and user identities remain out of scope for this
candidate.

## Client behavior

- The native owned launcher receives `session_token` over the private ready
  pipe and sends it as `Authorization: Bearer …`. External desktop mode reads
  `PHOTO_CULLER_BEARER` and fails closed if it is absent. Debug output redacts
  the token.
- The browser asks for a one-use code in a Slint pairing overlay, keeps the
  returned bearer only in WASM memory, uses `credentials: omit`, and sends it in
  the Authorization header. A 401 clears the bearer and returns to pairing.
  Reloading the page loses the in-memory session and requires a fresh code.
- The CLI reads a pairing code from stdin, stores the returned bearer in an
  explicit credential file, can request a code for another client, and revokes
  the current token on logout. Unix files are mode 0600 and are opened without
  following symbolic links; Windows inherits the parent directory ACL.
  `PHOTO_CULLER_BEARER` is available for externally managed short-lived
  commands. The CLI refuses session secrets over non-loopback cleartext HTTP.
  Logout removes the local credential even when remote revocation cannot be
  confirmed, and reports that the remote session may remain valid until expiry.

The CLI file is not an OS keychain integration. Pairing-code output is a
one-time credential and must be handed only to the intended client. The
browser and desktop do not refresh sessions automatically; browser reload or
session expiry requires re-pairing. A manually started server must be run in an
interactive terminal to reveal its initial code.

## Evidence and remaining gates

Manual loopback runtime evidence covers missing credentials (401), pairing,
authorized reads, a second paired client, self-revocation (204), rejection of
the revoked token (401), isolation between clients, unknown routes (404), and
non-interactive startup withholding the pairing code. The CLI runtime smoke is
recorded in `docs/runtime-evidence/2026-11-21-session-auth-cli-smoke.md`.

Native and browser packages compile with the new transports; the browser
transport also compiles for `wasm32-unknown-unknown`. These checks and the CLI
smoke do not constitute visual acceptance of the actual native/browser
windows. No test suite or cross-platform CI was run on this candidate.

Still required before treating authentication as release-ready: actual native
and browser window acceptance; verification of every mounted route and
mutation against the router; missing, invalid, expired and store-failure
coverage; shutdown/revocation lifecycle acceptance; explicit credentials for
service-managed browser startup; CLI credential storage in the OS keychain or
an equivalent reviewed store; role provisioning; and platform-matrix CI.
