# Builds Coucou Lite and its hook relay and zips them for distribution.
#   pwsh lite/scripts/pack.ps1            (run from the windows/ folder)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
Set-Location $root

cargo build --release -p coucou-lite -p coucou-hook
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

$version = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"(.+)"').Matches[0].Groups[1].Value
$out = Join-Path $root "release"
$stage = Join-Path $out "CoucouLite"
Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $stage | Out-Null

Copy-Item target\release\coucou-lite.exe $stage
Copy-Item target\release\coucou-hook.exe $stage
Copy-Item lite\README.md $stage

$zip = Join-Path $out "Coucou-Lite-$version.zip"
Remove-Item $zip -ErrorAction SilentlyContinue
Compress-Archive -Path "$stage\*" -DestinationPath $zip
Get-ChildItem $stage, $zip | Format-Table Name, @{n = "MB"; e = { [math]::Round($_.Length / 1MB, 2) }}
Write-Host "-> $zip"
