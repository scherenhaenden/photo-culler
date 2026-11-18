[CmdletBinding()]
param([Parameter(Mandatory = $true, Position = 0)][string]$Origin)
$ErrorActionPreference = 'Stop'
if ($Origin -notmatch '^http://(127\.0\.0\.1|localhost):[1-9][0-9]*$') { throw 'Origin must be loopback HTTP with an assigned port.' }
$health = Invoke-RestMethod -Uri "$Origin/api/v1/health" -TimeoutSec 2
if ($health.status -ne 'ready') { throw 'Health endpoint did not report ready.' }
$gallery = Invoke-RestMethod -Uri "$Origin/api/v1/galleries/demo/photos" -TimeoutSec 2
if (-not ($gallery.items | Where-Object { $_.id -eq 'demo-001' -or $_.photo_id -eq 'demo-001' })) {
    if (($gallery | ConvertTo-Json -Depth 8 -Compress) -notmatch 'demo-001') { throw 'Demo photo missing from gallery.' }
}
$response = Invoke-WebRequest -Uri "$Origin/api/v1/photos/demo-001/thumbnail/800" -TimeoutSec 2 -UseBasicParsing
if ($response.StatusCode -ne 200 -or $response.Headers['Content-Type'] -notmatch '^image/jpeg') { throw 'Thumbnail endpoint did not return image/jpeg.' }
Write-Output 'API smoke passed (health, demo gallery, JPEG thumbnail); GUI remains a manual check.'
