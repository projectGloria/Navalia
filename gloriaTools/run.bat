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
# Description: Launch the compiled release navalia.exe (--build to rebuild first)
$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent $env:GLORIA_TOOL_DIR.TrimEnd('\', '/')
Set-Location -LiteralPath $RepoRoot

$rebuild = $false
foreach ($a in $args) { if ($a -eq '--build') { $rebuild = $true } }
$binPath = Join-Path $RepoRoot 'target\release\navalia.exe'
if ($rebuild -or -not (Test-Path -LiteralPath $binPath)) {
	Write-Host '[run] Building release binary...' -ForegroundColor Cyan
	& cargo build --release --bin navalia
	if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
if (-not (Test-Path -LiteralPath $binPath)) { throw "Release binary not found: $binPath" }
Write-Host "[run] Launching $binPath..." -ForegroundColor Green
Start-Process -FilePath $binPath -WorkingDirectory $RepoRoot
