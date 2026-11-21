# Rust CLI server integration

The binary registers HTTP-backed operational commands through `main.rs` and
uses a synchronous `ureq` 3 adapter. The default origin is
`http://127.0.0.1:8765`, matching the Rust server's configured default port.
Use `--server URL` to select another server. The adapter applies finite
connect, response-header, and response-body timeouts and preserves HTTP error
responses so the CLI can report the status and response body.

`cargo check --locked -p photo-culler-cli` passes as part of the candidate
check on the current macOS host. No live-server request or runtime acceptance
is claimed here.

## Supported server commands

| Command | HTTP route | Behavior and limits |
|---|---|---|
| `status` | `GET /api/v1/health` | Returns the server health response. |
| `active-gallery` | `GET /api/v1/galleries/active` | Returns the configured active gallery, if any. |
| `galleries` | `GET /api/v1/galleries` | Lists galleries. |
| `profiles` | `GET /api/v1/profiles` | Lists catalog profiles. |
| `photo PHOTO_ID` | `GET /api/v1/photos/{photo_id}` | Reads one photo's catalog detail. |
| `photos GALLERY_ID` | `GET /api/v1/galleries/{gallery_id}/photos` | Reads the gallery photo page using server defaults; pagination/filter options are not exposed. |
| `summary GALLERY_ID` | `GET /api/v1/galleries/{gallery_id}/summary` | Reads the gallery summary. |
| `sessions GALLERY_ID` | `GET /api/v1/galleries/{gallery_id}/sessions` | Lists existing sessions; session editing/creation is not exposed. |
| `groups GALLERY_ID` | `GET /api/v1/galleries/{gallery_id}/groups` | Lists existing groups; grouping jobs are not exposed. |
| `sources GALLERY_ID` | `GET /api/v2/galleries/{gallery_id}/sources` | Lists import sources for a gallery. |
| `estimate SOURCE_ID` | `POST /api/v2/import-estimates` | Estimates a recursive import; exclusion patterns are not exposed. |
| `import GALLERY_ID SOURCE_ID` | `POST /api/v2/galleries/{gallery_id}/imports` | Queues a recursive import with default exclusions. |
| `scan GALLERY_ID` | `POST /api/v2/galleries/{gallery_id}/rescan` | Requests a gallery rescan. |
| `jobs` | `GET /api/v2/import-jobs` | Lists jobs with server defaults; limit/active filters are not exposed. |
| `job JOB_ID` | `GET /api/v2/import-jobs/{job_id}` | Reads one import job. |
| `job JOB_ID pause\|resume\|cancel` | `POST /api/v2/import-jobs/{job_id}/{action}` | Requests that job action. |
| `revisions GALLERY_ID` | `GET /api/v2/scan-revisions?gallery_id={gallery_id}` | Lists revisions for one gallery; limit is not exposed. |
| `decision PHOTO_ID STATE REVISION` | `PUT /api/v1/photos/{photo_id}/decision` | Writes one revision-checked decision. States: `keep`, `alt`/`alternate`, `reject`, `undecided`. |

IDs in path positions are restricted by the CLI to ASCII letters, digits,
period, dash, and underscore. Import commands always request recursive mode
and do not yet expose server exclusion-pattern settings. `decision` creates a
unique request ID per invocation and requires the expected revision.

## Local and offline commands

- `analyze PATH [MAX_DIMENSION]` reads a local image and emits its current
  Rust image-engine feature object as JSON. It does not call an API, enqueue a
  server analysis, or claim analyzer parity with Python.
- `doctor` and `diagnostics` report the configured catalog path and attempt a
  health check. They exit successfully even when the server is offline.
- `backends` and `frontends` print the readiness values exposed by the core
  crate. They are local inventory output, not deployment or runtime checks.

## Unsupported commands

These legacy/roadmap operations have no implemented CLI integration with the
current Rust server and return a usage error instead of calling a guessed
route: `init`, `config`, `volumes`, `group`, `bursts`, `decisions`
(bulk), `preselect`, `verify`, `evaluate`, `report`, and `nas`. `analyze` is
available only in its local `analyze PATH` form. Pagination, configurable
import exclusions, bulk decisions, server-side analysis/group generation, and NAS
management are not exposed by this CLI.

## Options and errors

API options must appear before a server command. `--server URL` changes the
server origin; `--credential-file PATH` selects the bearer file used by the
HTTP adapter; `--dry-run` prints a mutation's method, path, and body without
sending it. Read commands still contact the server. `--catalog PATH` is
accepted only by `doctor`/`diagnostics`, where it is displayed as context; no
command opens that catalog. Local `analyze`, `backends`, and `frontends` take
no API options; prefixing one returns a clear usage error. Output-format and
verbosity flags are not implemented and are not accepted.

## Session credentials

- `--credential-file PATH auth pair` reads one 64-character, one-use pairing
  code from stdin, calls `POST /api/v1/session/pair`, and stores the returned
  bearer in `PATH`. The bearer itself is never accepted in argv or printed.
- `--credential-file PATH auth pairing-code` asks the server for another
  short-lived pairing code and prints that one-use code once for the intended
  client. Treat the output as a credential.
- `--credential-file PATH auth logout` revokes that server session and removes
  the local file after successful revocation.
- The CLI sends the bearer in `Authorization` and the configured server origin
  in `Origin`; it does not put credentials in URLs. Auth command errors use
  generic messages, while ordinary API commands may display the server's
  response body.
- Session tokens and pairing codes are refused over cleartext HTTP to a
  non-loopback IP. Remote authenticated origins must use HTTPS; the current
  server build itself still binds to loopback only.
- On Unix, the credential file must be a regular file with no group/other
  permissions; newly written files use mode `0600` and are opened without
  following symbolic links. On Windows, the file
  inherits its parent directory's ACL. `PHOTO_CULLER_BEARER` is supported for
  externally managed, short-lived invocations when no file is selected.

`auth logout` removes the local credential even if the server cannot be
reached. A non-zero result reports that remote revocation could not be
confirmed; that server-side session may remain valid until expiry.

This is a local file/ephemeral environment credential provider, not an OS
keychain integration. A bearer expires after the server's bounded session TTL;
logout revokes it immediately when the server is reachable.

Exit codes are 0 for success (including offline diagnostics), 2 for usage or
unsupported operations, 3 for transport/timeouts, and 4 for non-2xx server
responses. The latter includes the HTTP status and response body where
available.
