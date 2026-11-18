[CmdletBinding()]
param([Parameter(Mandatory = $true, Position = 0)][string]$DesktopExecutable)
$ErrorActionPreference = 'Stop'
if (-not (Test-Path -LiteralPath $DesktopExecutable -PathType Leaf)) { throw "Desktop executable missing: $DesktopExecutable" }
$process = Start-Process -FilePath $DesktopExecutable -PassThru
Write-Output "Started desktop process PID=$($process.Id) PATH=$DesktopExecutable"
Write-Output 'Wait for the Slint window, record the assigned API origin, then run probe.ps1.'
