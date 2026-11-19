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
  [DllImport("user32.dll", SetLastError=true)] public static extern bool SetProcessDpiAwarenessContext(IntPtr value);
  [DllImport("user32.dll", SetLastError=true)] private static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
  [DllImport("kernel32.dll")] public static extern IntPtr GetCurrentProcess();
  [DllImport("shcore.dll")] public static extern int GetProcessDpiAwareness(IntPtr process, out int awareness);
  public static int[] GetWindowBounds(IntPtr hWnd) {
    RECT rect;
    if (!GetWindowRect(hWnd, out rect)) return null;
    return new int[] { rect.Left, rect.Top, rect.Right, rect.Bottom };
  }
}
'@
# GetWindowRect and CopyFromScreen must use the same physical-pixel coordinate
# space. A DPI-unaware PowerShell host otherwise returns virtualized bounds,
# which creates an oversized screenshot with black margins on hosted runners.
[void][NativeWindow]::SetProcessDpiAwarenessContext([IntPtr](-4)) # PER_MONITOR_AWARE_V2
$processDpiAwareness = -1
$dpiQueryResult = [NativeWindow]::GetProcessDpiAwareness([NativeWindow]::GetCurrentProcess(), [ref]$processDpiAwareness)
if ($dpiQueryResult -ne 0 -or $processDpiAwareness -ne 2) {
  throw "PowerShell capture process is not per-monitor DPI aware (query=$dpiQueryResult awareness=$processDpiAwareness)."
}
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
  $virtualScreen = [System.Windows.Forms.SystemInformation]::VirtualScreen
  if ($bounds.Left -lt $virtualScreen.Left -or $bounds.Top -lt $virtualScreen.Top -or
      $bounds.Right -gt $virtualScreen.Right -or $bounds.Bottom -gt $virtualScreen.Bottom) {
    throw "Slint window bounds ($($bounds.Left),$($bounds.Top),$($bounds.Right),$($bounds.Bottom)) exceed physical virtual-screen bounds ($($virtualScreen.Left),$($virtualScreen.Top),$($virtualScreen.Right),$($virtualScreen.Bottom))."
  }
  if ($bounds.Width -lt 800 -or $bounds.Height -lt 500) {
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
  [ordered]@{
    dpi_awareness = 'per-monitor-v2'
    window_bounds = @($bounds.Left, $bounds.Top, $bounds.Right, $bounds.Bottom)
    virtual_screen_bounds = @($virtualScreen.Left, $virtualScreen.Top, $virtualScreen.Right, $virtualScreen.Bottom)
    screenshot_dimensions = @($bounds.Width, $bounds.Height)
  } | ConvertTo-Json -Compress
} else {
  if (-not [NativeWindow]::PostMessage($handle, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)) {
    throw "Could not send WM_CLOSE to desktop PID $ProcessId window $handle."
  }
  Write-Output "Sent WM_CLOSE to Slint desktop PID=$ProcessId window=$handle"
}
