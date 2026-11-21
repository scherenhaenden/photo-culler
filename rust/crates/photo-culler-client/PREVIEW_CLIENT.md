# Preview REST client

`demo::previews::PreviewClient` calls the same-origin
`GET /api/v1/photos/{photo_id}/preview?size={small|medium|large|full}` route.
It validates IDs before path construction, caps response bytes, rejects
redirects, requires `image/jpeg`, and returns typed HTTP errors. The client does
not persist or rewrite the original photo.

The endpoint is still a candidate and must be registered by the server
integrator. Its source decoder currently supports raster formats enabled in
the image crate. RAW embedded-preview extraction is not connected to this
endpoint.
