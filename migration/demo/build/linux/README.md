# Linux x86-64 demo artifact

`build.sh --build OUTPUT WORKSPACE_ROOT` compiles both packages for
`x86_64-unknown-linux-gnu` and stages them side by side. `--stage` accepts
prebuilt files to test structure and executable bits; all paths are quoted, so
directories with spaces work.

For a GTK/winit-backed Slint build, the Linux runner needs a C toolchain and the
development headers for X11 and Wayland (`libx11-dev`, `libxkbcommon-dev`,
`libwayland-dev`, `libxcb1-dev`, `libxrandr-dev`, `libxkbcommon-x11-dev`) in
addition to Rust's `x86_64-unknown-linux-gnu` target. At runtime, run inside an
active X11 or Wayland graphical session; build dependencies do not provide a
display server.

Run the local staging acceptance check with:

```sh
sh migration/demo/build/linux/test-stage.sh
```

The sprint base does not register the Slint/server packages, so this local
staging test does not constitute a Linux x86-64 build. The final cross-target
build must run on D34's Linux runner.
