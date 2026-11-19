[CmdletBinding()]
param(
  [Parameter(Mandatory=$true)][ValidateSet('Capture','Close')][string]$Mode,
  [Parameter(Mandatory=$true)][int]$ProcessId,
  [string]$OutputPath
)
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class NativeWindow {
  [StructLayout(LayoutKind.Sequential)] private struct RECT {
    public int Left; public int Top; public int Right; public int Bottom;
  }
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hWnd, uint Msg, IntPtr wParam, IntPtr lParam);
  [DllImport("user32.dll", SetLastError=true)] private static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
  public static int[] GetWindowBounds(IntPtr hWnd) {
    RECT rect;
    if (!GetWindowRect(hWnd, out rect)) return null;
    return new int[] { rect.Left, rect.Top, rect.Right, rect.Bottom };
  }
}
'@
$deadline = [DateTime]::UtcNow.AddSeconds(30)
$process = $null
do {
  $process = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
  if ($process -and $process.MainWindowHandle -ne [IntPtr]::Zero) { break }
  Start-Sleep -Milliseconds 200
} while ([DateTime]::UtcNow -lt $deadline)
if (-not $process -or $process.MainWindowHandle -eq [IntPtr]::Zero) {
  throw "No visible Slint top-level window for desktop PID $ProcessId."
}
$handle = $process.MainWindowHandle
[void][NativeWindow]::SetForegroundWindow($handle)
Start-Sleep -Milliseconds 500
if ($Mode -eq 'Capture') {
  if (-not $OutputPath) { throw 'OutputPath is required for Capture.' }
  Add-Type -AssemblyName System.Windows.Forms
  Add-Type -AssemblyName System.Drawing
  $windowRect = [NativeWindow]::GetWindowBounds($handle)
  if (-not $windowRect) { throw "Could not read bounds for Slint window $handle." }
  $bounds = [System.Drawing.Rectangle]::FromLTRB($windowRect[0], $windowRect[1], $windowRect[2], $windowRect[3])
  if ($bounds.Width -lt 1000 -or $bounds.Height -lt 650) {
    throw "Slint window bounds are too small to capture: $($bounds.Width)x$($bounds.Height)."
  }
  $bitmap = New-Object System.Drawing.Bitmap($bounds.Width, $bounds.Height)
  $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
  try {
    $graphics.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size)
    $bitmap.Save($OutputPath, [System.Drawing.Imaging.ImageFormat]::Png)
  } finally {
    $graphics.Dispose()
    $bitmap.Dispose()
  }
  Write-Output "Captured Slint desktop PID=$ProcessId window=$handle path=$OutputPath"
} else {
  if (-not [NativeWindow]::PostMessage($handle, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)) {
    throw "Could not send WM_CLOSE to desktop PID $ProcessId window $handle."
  }
  Write-Output "Sent WM_CLOSE to Slint desktop PID=$ProcessId window=$handle"
}
