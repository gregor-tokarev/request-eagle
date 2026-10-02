# Build the app and package it as a per-user Windows installer,
# dist\RequestEagle-<version>-x64-setup.exe.

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true

$Root = Split-Path -Parent $PSScriptRoot
$Version = $env:VERSION
if (-not $Version) {
    $Version = (Select-String -Path "$Root\crates\request-eagle\Cargo.toml" -Pattern '^version = "(.+)"$' |
        Select-Object -First 1).Matches.Groups[1].Value
}
$CargoProfile = if ($env:PROFILE) { $env:PROFILE } else { 'dist' }
$DistDir = if ($env:DIST_DIR) { $env:DIST_DIR } else { "$Root\dist" }

Set-Location $Root
cargo build --locked --profile $CargoProfile -p request-eagle

$BuildDir = if ($CargoProfile -eq 'dev') { 'debug' } else { $CargoProfile }
$Executable = "$Root\target\$BuildDir\request-eagle.exe"

$Iscc = if ($env:ISCC) { $env:ISCC } else { "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe" }
New-Item -ItemType Directory -Force -Path $DistDir | Out-Null
& $Iscc /Qp "/DAppVersion=$Version" "/DAppExe=$Executable" "/O$DistDir" "$Root\packaging\windows\request-eagle.iss"

Write-Output "Packaged $DistDir\RequestEagle-$Version-x64-setup.exe"
