# Preview module integration

At base `1871eb9`, the roadmap target branch has no `photo-culler-server`
crate, crate manifest, media root module, or shared server test harness. This
directory is therefore a dependency-injected module with standalone unit tests.
The integrator must register `media::previews` from the future server module
tree and connect:

- `PreviewDecoder` to the read-only, orientation-aware decode layer from task 26;
- `PreviewEncoder` to the image engine's bounded raster/JPEG encoder;
- `PreviewCache` to the disposable derived-output cache, keyed by every field
  in `PreviewCacheKey`;
- `process_next` to the server's bounded background worker.

The Python reference presets are small/256 px, medium/800 px, large/1600 px,
and full/3200 px, encoded as JPEG quality 85. The module clamps every output to
3200 px on its longest side, applies EXIF orientation first, and preserves the
aspect ratio. Visible jobs are selected ahead of background jobs; jobs with
identical identity, source revision, source hash, render version, orientation,
format, quality, and preset share one in-flight render. A source or rendering
change must produce a new key.

The decoder receives only a source path and a pixel ceiling. The encoder sees
only derived pixels. No API in this module opens a source for writing, and the
returned preview carries the original hash unchanged. Cache contents are
disposable derivatives, not catalog identity or source-of-truth data.

Run the module tests without the absent server harness from the repository
root:

```sh
rustc --edition=2021 --test rust/apps/photo-culler-server/src/media/previews/mod.rs -o /tmp/photo-culler-previews-tests
/tmp/photo-culler-previews-tests
```
