# Fill in the open file dialog the standalone shows. Development aid.
param([Parameter(Mandatory = $true)][string]$Path)

Add-Type @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class Dlg {
    public delegate bool Proc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumWindows(Proc p, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr h, Proc p, IntPtr l);
    [DllImport("user32.dll")] public static extern int GetClassName(IntPtr h, StringBuilder s, int c);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern bool SetWindowText(IntPtr h, string s);
    [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
}
"@

$target = (Get-Process -Name saempler | Select-Object -First 1).Id
$dialog = [IntPtr]::Zero

$find = [Dlg+Proc] {
    param($h, $l)
    $owner = 0
    [Dlg]::GetWindowThreadProcessId($h, [ref]$owner) | Out-Null
    if ($owner -ne $target -or -not [Dlg]::IsWindowVisible($h)) { return $true }
    $name = New-Object System.Text.StringBuilder 64
    [Dlg]::GetClassName($h, $name, 64) | Out-Null
    if ($name.ToString() -eq "#32770") { $script:dialog = $h; return $false }
    return $true
}

for ($try = 0; $try -lt 20 -and $dialog -eq [IntPtr]::Zero; $try++) {
    [Dlg]::EnumWindows($find, [IntPtr]::Zero) | Out-Null
    if ($dialog -eq [IntPtr]::Zero) { Start-Sleep -Milliseconds 300 }
}
if ($dialog -eq [IntPtr]::Zero) { Write-Output "no dialog"; exit 1 }

# The file name box is the only edit control directly in the dialog tree.
$edit = [IntPtr]::Zero
$walk = [Dlg+Proc] {
    param($h, $l)
    $name = New-Object System.Text.StringBuilder 64
    [Dlg]::GetClassName($h, $name, 64) | Out-Null
    if ($name.ToString() -eq "Edit" -and $script:edit -eq [IntPtr]::Zero) { $script:edit = $h }
    return $true
}
[Dlg]::EnumChildWindows($dialog, $walk, [IntPtr]::Zero) | Out-Null
if ($edit -eq [IntPtr]::Zero) { Write-Output "no edit box"; exit 1 }

[Dlg]::SetWindowText($edit, $Path) | Out-Null
Start-Sleep -Milliseconds 400
# Return in the file name box, which the modern dialog honours where a posted
# IDOK does not.
[Dlg]::SendMessage($edit, 0x0100, [IntPtr]0x0D, [IntPtr]::Zero) | Out-Null
Start-Sleep -Milliseconds 80
[Dlg]::SendMessage($edit, 0x0102, [IntPtr]0x0D, [IntPtr]::Zero) | Out-Null
Start-Sleep -Milliseconds 80
[Dlg]::SendMessage($edit, 0x0101, [IntPtr]0x0D, [IntPtr]::Zero) | Out-Null
Write-Output "opened $Path"
