# Build Swapbar for Windows and pack it as target\Swapbar-windows-x64.zip, with its .sha256.
# Needs Rust and the Visual Studio Build Tools (MSVC and the Windows SDK, for rc.exe and fxc.exe).
#
#   ./scripts/package-windows.ps1
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot)

cargo build --release --locked
if ($LASTEXITCODE) { exit $LASTEXITCODE }

$stage = 'target\windows-package'
Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory $stage | Out-Null
Copy-Item 'target\release\swapbar.exe' "$stage\Swapbar.exe"

$zip = 'target\Swapbar-windows-x64.zip'
Remove-Item $zip -ErrorAction SilentlyContinue
Compress-Archive -Path "$stage\Swapbar.exe" -DestinationPath $zip
$hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
"$hash  Swapbar-windows-x64.zip" | Set-Content -Encoding ascii "$zip.sha256"
"wrote $zip ($((Get-Item $zip).Length) bytes, sha256 $hash)"
