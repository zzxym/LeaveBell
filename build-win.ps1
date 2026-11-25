# LeaveBell Native - Windows Build Script
param(
    [string]$Target = "",
    [string]$Arch = "amd64"
)

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

$exeName = "LeaveBell"

Write-Host "== 1/3 Building release =="
$buildArgs = @("build", "--release")
if ($Target) { $buildArgs += @("--target", $Target) }
& cargo @buildArgs
if ($LASTEXITCODE -ne 0) { Write-Error "Build failed"; exit 1 }

$root = $PSScriptRoot
$release = "E:\rust-target\release\$exeName.exe"
if ($Target) { $release = "$PSScriptRoot\target\$Target\release\$exeName.exe" }
if (-not (Test-Path $release)) { $release = "$PSScriptRoot\target\release\$exeName.exe" }
if (-not (Test-Path $release)) { Write-Error "Release exe not found"; exit 1 }

$out = Join-Path $PSScriptRoot "dist-native\school-display-$Arch"
# 基础版只含 edge-tts（在线），不打包 piper；它由本地语音包补丁提供
if (Test-Path $out) { Remove-Item $out -Recurse -Force }
New-Item -ItemType Directory -Path $out -Force | Out-Null
New-Item -ItemType Directory -Path (Join-Path $out "web") -Force | Out-Null
New-Item -ItemType Directory -Path (Join-Path $out "data") -Force | Out-Null
$presetData = Join-Path $PSScriptRoot "dist\school-display\data\data.json"
if (-not (Test-Path $presetData)) { $presetData = Join-Path $PSScriptRoot "preset\data.json" }
if (Test-Path $presetData) {
    Copy-Item $presetData (Join-Path $out "data\data.json") -Force
} else {
    # 首次启动由 Store::load 自动补齐全部默认字段
    [System.IO.File]::WriteAllText((Join-Path $out "data\data.json"), "{}", (New-Object System.Text.UTF8Encoding($false)))
}

Write-Host "== 2/3 Copying files =="
Copy-Item $release (Join-Path $out "$exeName.exe")
Copy-Item (Join-Path $root "assets\OFL-Noto-CJK.txt") (Join-Path $out "OFL-Noto-CJK.txt")
Copy-Item (Join-Path $root "web\*") (Join-Path $out "web") -Recurse -Force

Write-Host "== 3/3 Done =="
$size = (Get-ChildItem $out -Recurse | Measure-Object -Property Length -Sum).Sum / 1MB
Write-Host ("Output: {0}" -f $out)
Write-Host ("Total size: {0:N1} MB" -f $size)
Write-Host ("Run {0}\$exeName.exe" -f $out)
