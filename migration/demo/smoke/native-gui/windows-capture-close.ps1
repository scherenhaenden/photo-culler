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
  [DllImport("user32.dll", SetLastError=true)] private static extern bool SetWindowPos(IntPtr hWnd, IntPtr insertAfter, int x, int y, int width, int height, uint flags);
  [DllImport("user32.dll", SetLastError=true)] private static extern bool SystemParametersInfo(uint action, uint parameter, out RECT value, uint flags);
  [DllImport("dwmapi.dll")] private static extern int DwmGetWindowAttribute(IntPtr hWnd, uint attribute, out RECT value, uint size);
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
  public static int[] GetVisibleFrameBounds(IntPtr hWnd) {
    RECT rect;
    int result = DwmGetWindowAttribute(hWnd, 9, out rect, (uint)Marshal.SizeOf(typeof(RECT)));
    if (result != 0) return null; // DWMWA_EXTENDED_FRAME_BOUNDS
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
  public static int[] GetPrimaryWorkAreaBounds() {
    RECT rect;
    if (!SystemParametersInfo(0x0030, 0, out rect, 0)) return null; // SPI_GETWORKAREA
    return new int[] { rect.Left, rect.Top, rect.Right, rect.Bottom };
  }
  public static bool MoveWindowWithoutResize(IntPtr hWnd, int x, int y) {
    const uint SWP_NOSIZE = 0x0001;
    const uint SWP_NOZORDER = 0x0004;
    const uint SWP_NOACTIVATE = 0x0010;
    return SetWindowPos(hWnd, IntPtr.Zero, x, y, 0, 0,
      SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
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
  $workArea = [NativeWindow]::GetPrimaryWorkAreaBounds()
  if (-not $workArea) { throw 'Could not read physical primary work-area bounds.' }
  $originalWidth = $windowRect[2] - $windowRect[0]
  $originalHeight = $windowRect[3] - $windowRect[1]
  $targetLeft = $workArea[0] + 8
  $targetTop = $workArea[1] + 8
  if ($targetLeft + $originalWidth -gt $workArea[2] -or
      $targetTop + $originalHeight -gt $workArea[3]) {
    throw "Slint window ${originalWidth}x${originalHeight} does not fit in the primary work area ($($workArea -join ','))."
  }
  if (-not [NativeWindow]::MoveWindowWithoutResize($handle, $targetLeft, $targetTop)) {
    throw "Could not move Slint window $handle into the visible work area."
  }
  Start-Sleep -Milliseconds 300
  $windowRect = [NativeWindow]::GetWindowBounds($handle)
  if (-not $windowRect) { throw "Could not reread bounds for Slint window $handle after repositioning." }
  $windowRectBounds = [System.Drawing.Rectangle]::FromLTRB($windowRect[0], $windowRect[1], $windowRect[2], $windowRect[3])
  if ($windowRectBounds.Width -ne $originalWidth -or $windowRectBounds.Height -ne $originalHeight) {
    throw "Repositioning unexpectedly resized the Slint window from ${originalWidth}x${originalHeight} to $($windowRectBounds.Width)x$($windowRectBounds.Height)."
  }
  $visibleRect = [NativeWindow]::GetVisibleFrameBounds($handle)
  if (-not $visibleRect) { throw 'Could not read the DWM visible-frame bounds for the Slint window.' }
  $bounds = [System.Drawing.Rectangle]::FromLTRB($visibleRect[0], $visibleRect[1], $visibleRect[2], $visibleRect[3])
  if ($windowRectBounds.Left -lt $workArea[0] -or $windowRectBounds.Top -lt $workArea[1] -or
      $windowRectBounds.Right -gt $workArea[2] -or $windowRectBounds.Bottom -gt $workArea[3]) {
    throw "Slint GetWindowRect bounds ($($windowRectBounds.Left),$($windowRectBounds.Top),$($windowRectBounds.Right),$($windowRectBounds.Bottom)) exceed the primary work area ($($workArea -join ','))."
  }
  if ($bounds.Left -lt $workArea[0] -or $bounds.Top -lt $workArea[1] -or
      $bounds.Right -gt $workArea[2] -or $bounds.Bottom -gt $workArea[3]) {
    throw "Slint DWM visible-frame bounds ($($bounds.Left),$($bounds.Top),$($bounds.Right),$($bounds.Bottom)) exceed the primary work area ($($workArea -join ','))."
  }
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
    window_rect_bounds = @($windowRectBounds.Left, $windowRectBounds.Top, $windowRectBounds.Right, $windowRectBounds.Bottom)
    work_area_bounds = @($workArea[0], $workArea[1], $workArea[2], $workArea[3])
    physical_screen_bounds = @($physicalScreen[0], $physicalScreen[1], $physicalScreen[2], $physicalScreen[3])
    screenshot_dimensions = @($bounds.Width, $bounds.Height)
  } | ConvertTo-Json -Compress
} else {
  if (-not [NativeWindow]::PostMessage($handle, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)) {
    throw "Could not send WM_CLOSE to desktop PID $ProcessId window $handle."
  }
  Write-Output "Sent WM_CLOSE to Slint desktop PID=$ProcessId window=$handle"
}
