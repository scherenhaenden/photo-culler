[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$root = Join-Path ([System.IO.Path]::GetTempPath()) ('Photo Culler windows stage ' + [guid]::NewGuid().ToString('N'))
$input = Join-Path $root 'input files'
$output = Join-Path $root 'output folder with spaces'
try {
    New-Item -ItemType Directory -Force -Path $input | Out-Null
    Set-Content -LiteralPath (Join-Path $input 'desktop.exe') -Value 'mock desktop' -NoNewline
    Set-Content -LiteralPath (Join-Path $input 'server.exe') -Value 'mock server' -NoNewline
    & (Join-Path $here 'build.ps1') -Mode Stage -OutputDirectory $output -DesktopBinary (Join-Path $input 'desktop.exe') -ServerBinary (Join-Path $input 'server.exe')
    if ($LASTEXITCODE -and $LASTEXITCODE -ne 0) { throw "stage script exited $LASTEXITCODE" }
    foreach ($name in @('photo-culler-desktop.exe', 'photo-culler-server.exe')) {
        $staged = Join-Path $output $name
        if (-not (Test-Path -LiteralPath $staged -PathType Leaf)) { throw "missing staged binary: $staged" }
        if ((Get-Content -LiteralPath $staged -Raw) -notmatch 'mock') { throw "staged content mismatch: $staged" }
    }
    Write-Output 'Windows staging acceptance passed (paths with spaces).'
}
finally {
    Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
}
