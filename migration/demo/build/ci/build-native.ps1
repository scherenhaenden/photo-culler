[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][ValidateSet('x86_64-pc-windows-msvc')][string]$Target,
    [Parameter(Mandatory = $true)][string]$OutputDirectory,
    [Parameter(Mandatory = $true)][string]$WorkspaceRoot
)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path -LiteralPath $WorkspaceRoot).Path
$verify = Join-Path $root 'migration/demo/build/ci/verify-packages.py'
& python $verify $root
if ($LASTEXITCODE -ne 0) { throw "Cargo package verification failed with exit code $LASTEXITCODE" }

$manifest = Join-Path $root 'rust/Cargo.toml'
$packages = @('photo-culler-api', 'photo-culler-client', 'photo-culler-server', 'photo-culler-slint')
foreach ($package in $packages) {
    & cargo build --locked --manifest-path $manifest --release --target $Target --package $package
    if ($LASTEXITCODE -ne 0) { throw "Cargo build failed for $package with exit code $LASTEXITCODE" }
}

$release = Join-Path $root "rust/target/$Target/release"
$desktop = Join-Path $release 'photo-culler-desktop.exe'
$server = Join-Path $release 'photo-culler-server.exe'
foreach ($file in @($desktop, $server)) {
    if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { throw "required package output is missing: $file" }
}
$stage = Join-Path $root 'migration/demo/build/windows/build.ps1'
& $stage -Mode Stage -OutputDirectory $OutputDirectory -DesktopBinary $desktop -ServerBinary $server
if ($LASTEXITCODE -ne 0) { throw "Windows staging failed with exit code $LASTEXITCODE" }
$provenance = Join-Path $root 'migration/demo/build/ci/write-provenance.py'
& python $provenance $root $OutputDirectory $Target compile-and-stage photo-culler-api,photo-culler-client,photo-culler-server,photo-culler-slint
if ($LASTEXITCODE -ne 0) { throw "artifact provenance failed with exit code $LASTEXITCODE" }
