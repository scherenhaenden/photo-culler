#!/bin/sh
set -eu
origin=${1:?usage: probe.sh http://127.0.0.1:PORT}
case "$origin" in http://127.0.0.1:[0-9]*|http://localhost:[0-9]*) ;; *) echo "origin must be loopback http with an assigned port" >&2; exit 2;; esac
command -v curl >/dev/null 2>&1 || { echo "curl is required" >&2; exit 2; }
health=$(curl --fail --silent --show-error --max-time 2 "$origin/api/v1/health")
printf '%s\n' "$health" | grep -Eq '"status"[[:space:]]*:[[:space:]]*"ready"' || { echo "health response is not ready" >&2; exit 1; }
gallery=$(curl --fail --silent --show-error --max-time 2 "$origin/api/v1/galleries/demo/photos")
printf '%s\n' "$gallery" | grep -q 'demo-001' || { echo "demo gallery item missing" >&2; exit 1; }
printf '%s\n' "$gallery" | grep -q '/api/v1/photos/demo-001/thumbnail/800' || { echo "thumbnail URL missing" >&2; exit 1; }
type=$(curl --fail --silent --show-error --max-time 2 --output /dev/null --write-out '%{content_type}' "$origin/api/v1/photos/demo-001/thumbnail/800")
case "$type" in image/jpeg*) ;; *) echo "expected image/jpeg, got: $type" >&2; exit 1;; esac
printf '%s\n' 'API smoke passed; graphical rendering and lifecycle remain manual checks.'
