# Windows x86-64 demo artifact

Run from a Visual Studio Build Tools environment with the MSVC C++ workload,
Windows SDK, and Rust target `x86_64-pc-windows-msvc` installed:

```powershell
.\migration\demo\build\windows\build.ps1 -Mode Build -WorkspaceRoot 'C:\work\Photo Culler' -OutputDirectory 'C:\out\Photo Culler Demo'
```

The script builds the Slint desktop and server packages and stages adjacent
`photo-culler-desktop.exe` and `photo-culler-server.exe`. To exercise staging
with mock files and paths containing spaces:

```powershell
.\migration\demo\build\windows\test-stage.ps1
```

The sprint base does not register the Slint/server Cargo packages, so the mock
staging check is not a Windows x86-64 build. Run the real build on D34's
Windows runner.
