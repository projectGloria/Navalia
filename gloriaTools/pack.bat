@echo off
setlocal
set "GLORIA_TOOL_DIR=%~dp0"
set "GLORIA_SELF=%~f0"
powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "$lines = [System.IO.File]::ReadAllLines($env:GLORIA_SELF, [System.Text.Encoding]::UTF8); $idx = [Array]::IndexOf($lines, '# --- POWERSHELL_START ---'); $code = ($lines[($idx+1)..($lines.Length-1)] -join \"`n\"); & ([scriptblock]::Create($code)) @args" -- %*
set "EXIT_CODE=%ERRORLEVEL%"
if "%EXIT_CODE%" NEQ "0" (
	echo.
	echo [gloriaTools] Command failed with exit code %EXIT_CODE%.
	echo %CMDCMDLINE% | findstr /I /C:"%~nx0" >nul && (
		if /I "%CI%" NEQ "true" pause
	)
)
exit /b %EXIT_CODE%
# --- POWERSHELL_START ---
# Description: Build release binary, stage, pack GloriaSetup installer, and verify /S --dry-run
$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent $env:GLORIA_TOOL_DIR.TrimEnd('\', '/')
Set-Location -LiteralPath $RepoRoot

$gloriaRaw = Get-Content -Raw -LiteralPath 'gloria.toml' -Encoding UTF8
if ($gloriaRaw -notmatch '(?m)^version\s*=\s*"([^"]+)"') { throw 'Could not parse version from gloria.toml' }
$version = $Matches[1]
$appName = if ($gloriaRaw -match '(?m)^name\s*=\s*"([^"]+)"') { $Matches[1] } else { 'Navalia' }
$mainExe = if ($gloriaRaw -match '(?m)^main_exe\s*=\s*"([^"]+)"') { $Matches[1] } else { 'navalia.exe' }

Write-Host "[pack] Validating gloria.toml (v$version)..." -ForegroundColor Cyan
& npx --yes gloria-setup@latest --check --manifest gloria.toml --version $version
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host '[pack] Building release binary...' -ForegroundColor Cyan
& cargo build --release --bin navalia
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$binPath = Join-Path $RepoRoot "target\release\$mainExe"
if (-not (Test-Path -LiteralPath $binPath)) { throw "Built binary not found: $binPath" }

$stageDir = Join-Path $RepoRoot 'stage'
if (Test-Path -LiteralPath $stageDir) { Remove-Item -LiteralPath $stageDir -Recurse -Force }
New-Item -ItemType Directory -Path $stageDir | Out-Null
Copy-Item -LiteralPath $binPath -Destination (Join-Path $stageDir $mainExe) -Force

$distDir = Join-Path $RepoRoot 'dist'
New-Item -ItemType Directory -Force -Path $distDir | Out-Null

Write-Host '[pack] Packing installer via gloria-setup@latest...' -ForegroundColor Cyan
& npx --yes gloria-setup@latest --app $stageDir --manifest gloria.toml --version $version --out $distDir
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$setupExe = Join-Path $distDir "${appName}_${version}_x64-setup.exe"
if (-not (Test-Path -LiteralPath $setupExe)) { throw "Expected installer not found: $setupExe" }

Write-Host "[pack] Verifying $setupExe with /S --dry-run..." -ForegroundColor Cyan
$proc = Start-Process -FilePath $setupExe -ArgumentList '/S --dry-run' -Wait -PassThru
if ($proc.ExitCode -ne 0) { throw "Installer dry-run failed with exit code $($proc.ExitCode)" }
Write-Host "[pack] Success: $setupExe (dry-run ExitCode=0)" -ForegroundColor Green
