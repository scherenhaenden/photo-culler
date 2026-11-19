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
  [DllImport("user32.dll", SetLastError=true)] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr value);
  [DllImport("user32.dll")] public static extern IntPtr GetThreadDpiAwarenessContext();
  [DllImport("user32.dll")] public static extern int GetAwarenessFromDpiAwarenessContext(IntPtr value);
  [DllImport("user32.dll")] private static extern IntPtr GetDC(IntPtr hWnd);
  [DllImport("user32.dll")] private static extern int ReleaseDC(IntPtr hWnd, IntPtr hDC);
  [DllImport("gdi32.dll")] private static extern int GetDeviceCaps(IntPtr hDC, int index);
  [DllImport("user32.dll", SetLastError=true)] private static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
  public static int[] GetWindowBounds(IntPtr hWnd) {
    RECT rect;
    if (!GetWindowRect(hWnd, out rect)) return null;
    return new int[] { rect.Left, rect.Top, rect.Right, rect.Bottom };
  }
  public static int[] GetPhysicalPrimaryScreenBounds() {
    IntPtr hdc = GetDC(IntPtr.Zero);
    if (hdc == IntPtr.Zero) return null;
    try {
      int width = GetDeviceCaps(hdc, 8);  // HORZRES, physical display pixels
      int height = GetDeviceCaps(hdc, 10); // VERTRES, physical display pixels
      if (width <= 0 || height <= 0) return null;
      return new int[] { 0, 0, width, height };
    } finally {
      ReleaseDC(IntPtr.Zero, hdc);
    }
  }
}
'@
# GetWindowRect and CopyFromScreen must use the same physical-pixel coordinate
# space. A thread override works even when the PowerShell process manifest has
# already fixed its process-wide DPI mode to system-aware.
$previousDpiContext = [NativeWindow]::SetThreadDpiAwarenessContext([IntPtr](-4)) # PER_MONITOR_AWARE_V2
$threadDpiAwareness = [NativeWindow]::GetAwarenessFromDpiAwarenessContext([NativeWindow]::GetThreadDpiAwarenessContext())
if ($previousDpiContext -eq [IntPtr]::Zero -or $threadDpiAwareness -ne 2) {
  throw "PowerShell capture thread is not per-monitor DPI aware (awareness=$threadDpiAwareness)."
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
  Add-Type -AssemblyName System.Drawing
  $windowRect = [NativeWindow]::GetWindowBounds($handle)
  if (-not $windowRect) { throw "Could not read bounds for Slint window $handle." }
  $bounds = [System.Drawing.Rectangle]::FromLTRB($windowRect[0], $windowRect[1], $windowRect[2], $windowRect[3])
  $physicalScreen = [NativeWindow]::GetPhysicalPrimaryScreenBounds()
  if (-not $physicalScreen) { throw 'Could not read physical primary-screen pixel bounds.' }
  if ($bounds.Left -lt $physicalScreen[0] -or $bounds.Top -lt $physicalScreen[1] -or
      $bounds.Right -gt $physicalScreen[2] -or $bounds.Bottom -gt $physicalScreen[3]) {
    throw "Slint window bounds ($($bounds.Left),$($bounds.Top),$($bounds.Right),$($bounds.Bottom)) exceed physical primary-screen bounds (0,0,$($physicalScreen[2]),$($physicalScreen[3]))."
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
    dpi_awareness = 'thread-per-monitor-v2'
    window_bounds = @($bounds.Left, $bounds.Top, $bounds.Right, $bounds.Bottom)
    physical_screen_bounds = @($physicalScreen[0], $physicalScreen[1], $physicalScreen[2], $physicalScreen[3])
    screenshot_dimensions = @($bounds.Width, $bounds.Height)
  } | ConvertTo-Json -Compress
} else {
  if (-not [NativeWindow]::PostMessage($handle, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)) {
    throw "Could not send WM_CLOSE to desktop PID $ProcessId window $handle."
  }
  Write-Output "Sent WM_CLOSE to Slint desktop PID=$ProcessId window=$handle"
}
