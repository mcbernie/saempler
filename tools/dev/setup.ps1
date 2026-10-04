# Bring a freshly started standalone into a state worth looking at: a sample
# loaded, sliced and mapped across the keyboard. Development aid, not shipped.
#
#   .\tools\dev\setup.ps1 -Sample "C:\pfad\zum\sample.wav"
#
# The coordinates below are the editor's own, at its default size. They are
# read off the interface and have to be corrected whenever the layout moves.
param(
    [Parameter(Mandatory = $true)][string]$Sample,
    [string]$Out = "target/setup.png",
    [ValidateSet(4, 8, 16, 20)][int]$Slices = 16
)

$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$shot = Join-Path $here 'shot.ps1'
$dialog = Join-Path $here 'dialog.ps1'

# SOURCE SAMPLE toolbar: the folder icon, then the even divisions.
$folder = @(193, 84)
$divisions = @{ 4 = 245; 8 = 289; 16 = 332; 20 = 375 }
$divideY = 84
# PERFORMANCE toolbar: lay every slice on a note.
$mapToKeys = @(37, 281)

Write-Host "Dateidialog oeffnen"
& $shot -Out $Out -Click $folder | Out-Null
Start-Sleep -Seconds 3

Write-Host "Sample waehlen: $Sample"
& $dialog -Path $Sample
Start-Sleep -Seconds 5

Write-Host "In $Slices Teile schneiden"
& $shot -Out $Out -Click @($divisions[$Slices], $divideY) | Out-Null
Start-Sleep -Seconds 2

Write-Host "Auf die Tastatur legen"
& $shot -Out $Out -Click $mapToKeys | Out-Null
Start-Sleep -Seconds 2

& $shot -Out $Out
