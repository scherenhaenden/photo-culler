[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][ValidateSet('Build', 'Stage')][string]$Mode,
    [Parameter(Mandatory = $true)][string]$OutputDirectory,
    [string]$WorkspaceRoot,
    [string]$DesktopBinary,
    [string]$ServerBinary
)
$ErrorActionPreference = 'Stop'

if ($Mode -eq 'Build') {
    if (-not $WorkspaceRoot) { throw 'WorkspaceRoot is required in Build mode.' }
    $manifest = Join-Path $WorkspaceRoot 'rust/Cargo.toml'
    & cargo build --manifest-path $manifest --release --target x86_64-pc-windows-msvc -p photo-culler-slint -p photo-culler-server
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed with exit code $LASTEXITCODE" }
    $release = Join-Path $WorkspaceRoot 'rust/target/x86_64-pc-windows-msvc/release'
    $DesktopBinary = Join-Path $release 'photo-culler-desktop.exe'
    $ServerBinary = Join-Path $release 'photo-culler-server.exe'
}
if (-not $DesktopBinary -or -not $ServerBinary) {
    throw 'DesktopBinary and ServerBinary are required in Stage mode.'
}
foreach ($file in @($DesktopBinary, $ServerBinary)) {
    if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { throw "Executable file missing: $file" }
}
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
Copy-Item -LiteralPath $DesktopBinary -Destination (Join-Path $OutputDirectory 'photo-culler-desktop.exe') -Force
Copy-Item -LiteralPath $ServerBinary -Destination (Join-Path $OutputDirectory 'photo-culler-server.exe') -Force
Write-Output "Staged Windows x86-64 demo in: $OutputDirectory"
