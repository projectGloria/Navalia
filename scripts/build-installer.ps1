# Builds the GloriaSetup installer: dist/Navalia_<version>_x64-setup.exe.
# Usage: scripts/build-installer.ps1   (from any folder)

$ErrorActionPreference = 'Continue'
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    $meta = cargo metadata --format-version 1 --no-deps | ConvertFrom-Json
    $target = $meta.target_directory
    $version = ($meta.packages | Where-Object name -eq 'navalia' | Select-Object -ExpandProperty version)

    Push-Location (Join-Path $root 'apps/settings')
    try {
        npx tsc --noEmit
        if ($LASTEXITCODE) { throw 'tsc failed' }
        npx vite build
        if ($LASTEXITCODE) { throw 'vite build failed' }
    } finally { Pop-Location }

    cargo build --release -p navalia -p navalia-settings
    if ($LASTEXITCODE) { throw 'cargo build failed' }

    $stage = Join-Path $root 'stage'
    New-Item -ItemType Directory -Force $stage -ErrorAction Stop | Out-Null
    Copy-Item (Join-Path $target 'release/navalia.exe') (Join-Path $stage 'navalia.exe') -Force -ErrorAction Stop
    Copy-Item (Join-Path $target 'release/navalia-settings.exe') (Join-Path $stage 'navalia-settings.exe') -Force -ErrorAction Stop

    npx --yes gloria-setup@latest --app $stage --manifest (Join-Path $root 'gloria.toml') --version $version --out (Join-Path $root 'dist')
    if ($LASTEXITCODE) { throw 'gloria-setup failed' }

    Get-ChildItem (Join-Path $root 'dist/*_x64-setup.exe') |
        Select-Object FullName, @{ n = 'MB'; e = { [math]::Round($_.Length / 1MB, 2) } }
} finally { Pop-Location }
