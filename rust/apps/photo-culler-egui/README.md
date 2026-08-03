# Native egui + wgpu desktop client

This native desktop client renders with `eframe` on the `wgpu` backend. It
uses the versioned local application API rather than opening SQLite directly,
so it shares catalog identity, persistent import jobs, analysis profiles and
non-destructive decisions with the Web UI.

## Available workflow

- connect to a local Photo Culler service;
- browse galleries and the active catalog, including thumbnail preview;
- create a gallery and queue an in-place folder import;
- start, pause, resume and cancel analysis while polling its progress; and
- persist `best`, `keep`, `review` and `reject` decisions.

Start the service, then launch the native client:

```bash
photo-culler web --no-open
cd rust
cargo run -p photo-culler-egui
```

The service URL is editable in the app and defaults to `http://127.0.0.1:8765`.
The Python application service remains the owner of the catalog and analysis
pipeline; the native client is deliberately a delivery adapter, not a second
database implementation.

## Readiness calculation

The current functional-readiness score is **100% (10/10) for the documented Linux
experimental scope**. The package includes the native launcher; its process E2E
verifies the local service, catalog/log persistence and clean shutdown.
