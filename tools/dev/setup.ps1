# Bring the freshly started standalone into a state worth looking at:
# a sample loaded, sliced and mapped across the keyboard. Development aid.
param([string]$Out = "target/shot.png", [int]$Tab = 0)

$shot = "C:\Users\nbrue\dev\saempler\target\shot.ps1"
# SLICE / SOUND, SOURCE, MODIFIERS
$tabs = @(570, 666, 753)
$tabY = 211

# Open the source page and load a file.
& $shot -Out target/setup.png -Click @($tabs[1], $tabY, 590, 290) | Out-Null
Start-Sleep -Seconds 1
& $shot -Out target/setup.png -Click @(250, 409, 1015, 650) | Out-Null
Start-Sleep -Seconds 2
# Cut into sixteen, then lay them across the keyboard.
& $shot -Out target/setup.png -Click @(778, 290) | Out-Null
& $shot -Out $Out -Click @(100, 290, $tabs[$Tab], $tabY)
