# LeaveBell Native - Windows Build Script
param(
    [string]$Target = "",
    [string]$Arch = "amd64"
)

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

Write-Host "== 1/3 Building release =="
$buildArgs = @("build", "--release")
if ($Target) { $buildArgs += @("--target", $Target) }
& cargo @buildArgs
if ($LASTEXITCODE -ne 0) { Write-Error "Build failed"; exit 1 }

$root = $PSScriptRoot
$release = Join-Path $root "target\release\LeaveBell.exe"
if ($Target) { $release = Join-Path $root "target\$Target\release\LeaveBell.exe" }
if (-not (Test-Path $release)) { $release = Join-Path $root "target\release\LeaveBell.exe" }
if (-not (Test-Path $release)) { Write-Error "Release exe not found"; exit 1 }
$displayRelease = Join-Path (Split-Path $release) "LeaveBellDisplay.exe"
if (-not (Test-Path $displayRelease)) { Write-Error "Display exe not found"; exit 1 }

$online = Join-Path $root "dist-native\online-$Arch"
$screen = Join-Path $root "dist-native\screen-$Arch"
foreach ($out in @($online, $screen)) {
    if (Test-Path $out) { Remove-Item $out -Recurse -Force }
    New-Item -ItemType Directory -Path $out -Force | Out-Null
}
New-Item -ItemType Directory -Path (Join-Path $online "web") -Force | Out-Null
New-Item -ItemType Directory -Path (Join-Path $online "data") -Force | Out-Null

$presetData = Join-Path $root "preset\data.json"
Copy-Item $presetData (Join-Path $online "data\data.json") -Force

Write-Host "== 2/3 Copying files =="
Copy-Item $release (Join-Path $online "LeaveBell.exe")
Copy-Item $displayRelease (Join-Path $online "LeaveBellDisplay.exe")
Copy-Item (Join-Path $root "assets\OFL-Noto-CJK.txt") (Join-Path $online "OFL-Noto-CJK.txt")
Copy-Item (Join-Path $root "web\*") (Join-Path $online "web") -Recurse -Force

Copy-Item $displayRelease (Join-Path $screen "LeaveBellDisplay.exe")
Copy-Item (Join-Path $root "assets\OFL-Noto-CJK.txt") (Join-Path $screen "OFL-Noto-CJK.txt")

Write-Host "== 3/3 Done =="
Write-Host "Online payload: $online"
Write-Host "Screen payload: $screen"
