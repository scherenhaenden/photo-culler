# macOS arm64 demo artifact

`build.sh --build OUTPUT WORKSPACE_ROOT` builds release binaries for
`aarch64-apple-darwin` and stages `photo-culler-desktop` next to
`photo-culler-server`. `build.sh --stage OUTPUT DESKTOP SERVER` stages supplied
binaries and is used to verify layout and executable permissions before the
real packages exist. All paths are quoted, including output folders containing
spaces.

Run the staging acceptance check with:

```sh
sh migration/demo/build/macos/test-stage.sh
```

The current sprint base does not register `photo-culler-slint` or
`photo-culler-server` as Cargo packages, so this staging check does not claim
that an arm64 application build ran. Once D34 provides the packages and the
`aarch64-apple-darwin` target, run the `--build` form on an Apple Silicon Mac.
