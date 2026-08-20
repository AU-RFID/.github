# RFID Lab onboarding TUI — Windows host bootstrap
#
# Downloads the prebuilt onboarding binary from the latest onboarding-tui-v*
# GitHub Release and runs it on the Windows host. The TUI will ask which WSL
# distro to install dev tools into; GUI apps install on Windows via winget.
#
#   irm https://raw.githubusercontent.com/AU-RFID/.github/main/public-scripts/onboard.ps1 | iex
#
$ErrorActionPreference = "Stop"

$Repo = "AU-RFID/.github"
$BinDir = Join-Path $env:LOCALAPPDATA "rfid-onboard"
$Bin = Join-Path $BinDir "onboarding-tui.exe"

if (-not (Test-Path $Bin)) {
    New-Item -ItemType Directory -Force -Path $BinDir | Out-Null
    $releases = Invoke-RestMethod "https://api.github.com/repos/$Repo/releases"
    $tag = ($releases | Where-Object { $_.tag_name -like "onboarding-tui-v*" } |
        Select-Object -First 1).tag_name
    if (-not $tag) {
        Write-Error "No onboarding-tui release found in $Repo."
    }
    $url = "https://github.com/$Repo/releases/download/$tag/onboarding-tui-x86_64-pc-windows-msvc.exe"
    Write-Host "Downloading onboarding TUI $tag..."
    Invoke-WebRequest -Uri $url -OutFile $Bin
}

& $Bin @args
