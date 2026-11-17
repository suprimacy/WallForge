# Build WallForge: the React interface plus the Rust (Tauri) app.
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

# Native tools write progress to stderr; keep that from aborting the script.
$nativePref = $ErrorActionPreference
$ErrorActionPreference = "Continue"

# The interface is bundled into the app, so build it first.
if (Get-Command npm -ErrorAction SilentlyContinue) {
    Write-Host "Building the web interface..."
    Push-Location (Join-Path $PSScriptRoot "webapp")
    npm install --no-audit --no-fund
    if ($LASTEXITCODE -ne 0) { Pop-Location; throw "npm install failed" }
    npm run build
    if ($LASTEXITCODE -ne 0) { Pop-Location; throw "The web interface failed to build" }
    Pop-Location
} else {
    Write-Warning "npm was not found, so the web interface was not rebuilt."
}

# The Rust core.
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw "Rust is not installed. Get it from https://rustup.rs and run this script again."
}

Write-Host "Building the Rust app..."
Push-Location (Join-Path $PSScriptRoot "webapp\src-tauri")
cargo build --release
$buildCode = $LASTEXITCODE
Pop-Location
$ErrorActionPreference = $nativePref
if ($buildCode -ne 0) { throw "The Rust build failed" }

$exe = Join-Path $PSScriptRoot "webapp\src-tauri\target\release\wallforge.exe"
Write-Host ""
Write-Host "Built $exe"
Get-Item $exe |
    Select-Object Name, @{ n = "SizeMB"; e = { [math]::Round($_.Length / 1MB, 1) } }, LastWriteTime |
    Format-Table -AutoSize

Write-Host ""
Write-Host "The app needs the DepotDownloaderMod folder next to it:"
Get-ChildItem (Join-Path $PSScriptRoot "webapp\src-tauri\target\release\DepotDownloaderMod") |
    Select-Object -ExpandProperty Name
