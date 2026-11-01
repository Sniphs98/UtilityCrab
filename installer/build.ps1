# Baut das Release und daraus das Setup (dist\UtilityCrab-Setup-<Version>.exe).
# Ohne -Version wird die Version aus Cargo.toml genommen.
param([string]$Version)
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

if (-not $Version) {
    $match = Select-String -Path "Cargo.toml" -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1
    $Version = $match.Matches[0].Groups[1].Value
}

cargo build --release
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$iscc = @(
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $iscc) {
    Write-Error "Inno Setup 6 nicht gefunden. Installieren mit: winget install JRSoftware.InnoSetup"
}

& $iscc "/DAppVersion=$Version" "installer\UtilityCrab.iss"
exit $LASTEXITCODE
