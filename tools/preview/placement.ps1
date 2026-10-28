$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class HerdrPreviewWindow {
  [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left, Top, Right, Bottom; }
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out Rect rect);
}
'@
$handle = [HerdrPreviewWindow]::GetForegroundWindow()
$rect = New-Object HerdrPreviewWindow+Rect
if (-not [HerdrPreviewWindow]::GetWindowRect($handle, [ref]$rect)) { throw 'Cannot read the active window.' }
$area = [System.Windows.Forms.Screen]::FromHandle($handle).WorkingArea
$width = [Math]::Min(640, $area.Width)
$height = [Math]::Min(900, $area.Height)
$x = if (($area.Right - $rect.Right) -ge $width) { $rect.Right } elseif (($rect.Left - $area.Left) -ge $width) { $rect.Left - $width } else { $area.Right - $width }
$y = [Math]::Max($area.Top, [Math]::Min($rect.Top, $area.Bottom - $height))
@{ x = $x; y = $y; width = $width; height = $height } | ConvertTo-Json -Compress
