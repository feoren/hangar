<#
.SYNOPSIS
  Captures only the Hangar window (never the whole screen).

.DESCRIPTION
  Brings the window to the front and saves its content as a PNG via PrintWindow. PrintWindow
  reads the window content itself, so other windows overlapping it never end up in the image.
  For agents: delete the images after viewing them; they can show the user's terminal content.

.EXAMPLE
  pwsh -File tools/screenshot-hangar.ps1 -Out "$env:TEMP\hangar.png"

.EXAMPLE
  # With a dev build running next to an installed Hangar, pick the window by process id.
  pwsh -File tools/screenshot-hangar.ps1 -Out "$env:TEMP\hangar.png" -ProcessId 4242
#>
param(
  [Parameter(Mandatory = $true)][string]$Out,
  [int]$ProcessId = 0
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class HangarWin {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
}
"@

$process = Get-Process hangar -ErrorAction SilentlyContinue |
  Where-Object { $_.MainWindowHandle -ne 0 -and ($ProcessId -eq 0 -or $_.Id -eq $ProcessId) } |
  Select-Object -First 1
if (-not $process) { throw "Hangar is not running (no window found)." }

$handle = $process.MainWindowHandle
[HangarWin]::ShowWindow($handle, 9) | Out-Null   # SW_RESTORE, in case it is minimized
[HangarWin]::SetForegroundWindow($handle) | Out-Null
Start-Sleep -Milliseconds 400

$rect = New-Object HangarWin+RECT
[HangarWin]::GetWindowRect($handle, [ref]$rect) | Out-Null
$width = $rect.R - $rect.L
$height = $rect.B - $rect.T
$bitmap = New-Object System.Drawing.Bitmap $width, $height
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$hdc = $graphics.GetHdc()
# 2 = PW_RENDERFULLCONTENT: required for WebView2 content, otherwise the image stays black.
[HangarWin]::PrintWindow($handle, $hdc, 2) | Out-Null
$graphics.ReleaseHdc($hdc)
$graphics.Dispose()
$bitmap.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$bitmap.Dispose()
Write-Output "$width x $height -> $Out"
