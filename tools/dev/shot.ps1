# Drive the standalone window and capture it. Development aid, not shipped.
#
# Input is injected as real mouse input, because the editor window is driven by
# baseview and posted messages do not reach egui's input state.
param(
    [string]$Out = "target/shot.png",
    [int[]]$Click = @(),
    [int[]]$Rclick = @(),
    [int[]]$Drag = @(),
    [int[]]$Wheel = @(),
    [int]$W = 0,
    [int]$H = 0
)

Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class Shot {
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out R r);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref P p);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int t, bool r);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll", SetLastError=true)] public static extern uint SendInput(uint n, INPUT[] i, int cb);
    [StructLayout(LayoutKind.Sequential)] public struct MOUSEINPUT {
        public int dx, dy; public uint mouseData, dwFlags, time; public IntPtr dwExtraInfo;
    }
    [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public MOUSEINPUT mi; }
    [StructLayout(LayoutKind.Sequential)] public struct R { public int L, T, Rr, B; }
    [StructLayout(LayoutKind.Sequential)] public struct P { public int X, Y; }
}
"@

$SIZE = [System.Runtime.InteropServices.Marshal]::SizeOf([type][Shot+INPUT])

# The MOUSEINPUT is filled in first and assigned whole: writing through
# $input.mi.dwFlags would set the field on a copy of the struct.
function Send-Button([uint32]$flag) {
    $mi = New-Object Shot+MOUSEINPUT
    $mi.dwFlags = $flag
    $i = New-Object Shot+INPUT
    $i.type = 0
    $i.mi = $mi
    [Shot]::SendInput(1, @($i), $SIZE) | Out-Null
}

$proc = Get-Process -Name saempler -ErrorAction SilentlyContinue |
    Where-Object { $_.MainWindowHandle -ne [IntPtr]::Zero } | Select-Object -First 1
if (-not $proc) { Write-Output "no window"; exit 1 }
$hwnd = $proc.MainWindowHandle

if ($W -gt 0) {
    [Shot]::MoveWindow($hwnd, 60, 60, $W, $H, $true) | Out-Null
    Start-Sleep -Milliseconds 600
}

[Shot]::SetForegroundWindow($hwnd) | Out-Null
Start-Sleep -Milliseconds 300

$rect = New-Object Shot+R
[Shot]::GetClientRect($hwnd, [ref]$rect) | Out-Null
$origin = New-Object Shot+P
[Shot]::ClientToScreen($hwnd, [ref]$origin) | Out-Null

function Move-To([int]$x, [int]$y) {
    [Shot]::SetCursorPos($origin.X + $x, $origin.Y + $y) | Out-Null
    Start-Sleep -Milliseconds 70
}

for ($i = 0; $i + 1 -lt $Click.Count; $i += 2) {
    Move-To $Click[$i] $Click[$i + 1]
    Send-Button 0x0002
    Start-Sleep -Milliseconds 90
    Send-Button 0x0004
    Start-Sleep -Milliseconds 350
}

for ($i = 0; $i + 1 -lt $Rclick.Count; $i += 2) {
    Move-To $Rclick[$i] $Rclick[$i + 1]
    Send-Button 0x0008
    Start-Sleep -Milliseconds 90
    Send-Button 0x0010
    Start-Sleep -Milliseconds 350
}

# A drag is given as x1 y1 x2 y2, moved in steps so the widget sees motion.
for ($i = 0; $i + 3 -lt $Drag.Count; $i += 4) {
    Move-To $Drag[$i] $Drag[$i + 1]
    Send-Button 0x0002
    Start-Sleep -Milliseconds 90
    for ($s = 1; $s -le 14; $s++) {
        Move-To ([int]($Drag[$i] + ($Drag[$i + 2] - $Drag[$i]) * $s / 14)) `
                ([int]($Drag[$i + 1] + ($Drag[$i + 3] - $Drag[$i + 1]) * $s / 14))
    }
    Send-Button 0x0004
    Start-Sleep -Milliseconds 350
}

# A wheel step is given as x y notches.
for ($i = 0; $i + 2 -lt $Wheel.Count; $i += 3) {
    Move-To $Wheel[$i] $Wheel[$i + 1]
    $mi = New-Object Shot+MOUSEINPUT
    $mi.dwFlags = 0x0800
    # A negative delta has to be sent as its unsigned two's complement.
    $delta = 120 * $Wheel[$i + 2]
    if ($delta -lt 0) { $delta = 4294967296 + $delta }
    $mi.mouseData = [uint32]$delta
    $input = New-Object Shot+INPUT
    $input.type = 0
    $input.mi = $mi
    [Shot]::SendInput(1, @($input), $SIZE) | Out-Null
    Start-Sleep -Milliseconds 250
}

# Park the cursor in a corner, so no control is caught in its hover state.
Move-To 2 2
Start-Sleep -Milliseconds 500

$width = $rect.Rr - $rect.L
$height = $rect.B - $rect.T
$bmp = New-Object System.Drawing.Bitmap $width, $height
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($origin.X, $origin.Y, 0, 0, (New-Object System.Drawing.Size $width, $height))
$bmp.Save((Resolve-Path .).Path + "\" + $Out.Replace("/", "\"))
Write-Output "$width x $height -> $Out"
