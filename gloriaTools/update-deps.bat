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
# Description: Audit outdated/vulnerable dependencies (--apply to update lockfiles)
$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent $env:GLORIA_TOOL_DIR.TrimEnd('\', '/')
Set-Location -LiteralPath $RepoRoot

$apply = $false
foreach ($a in $args) { if ($a -eq '--apply') { $apply = $true } }
if ($apply) {
& cargo update --manifest-path Cargo.toml; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
	Write-Host '[update-deps] Done.' -ForegroundColor Green
	exit 0
}
if (Get-Command cargo-outdated -ErrorAction SilentlyContinue) { Write-Host "`n=== cargo outdated ===" -ForegroundColor Cyan; & cargo outdated --manifest-path Cargo.toml; $global:LASTEXITCODE = 0 }
Write-Host "`nRun with --apply to update lockfiles." -ForegroundColor DarkGray
