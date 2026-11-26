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
# Description: Stop locked project processes and clean build/cache directories (--deep, --dry-run)
$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent $env:GLORIA_TOOL_DIR.TrimEnd('\', '/')
Set-Location -LiteralPath $RepoRoot

$deep = $false
$dryRun = $false
foreach ($a in $args) {
	if ($a -eq '--deep') { $deep = $true }
	elseif ($a -eq '--dry-run') { $dryRun = $true }
}
if (-not $dryRun) {
	$stopBat = Join-Path $env:GLORIA_TOOL_DIR 'stop.bat'
	if (Test-Path -LiteralPath $stopBat) { & cmd.exe /c "`"$stopBat`"" }
}
$targets = @('dist', 'stage', 'target')
if ($deep) {  }
foreach ($rel in $targets) {
	$full = Join-Path $RepoRoot $rel
	if (Test-Path -LiteralPath $full) {
		if ($dryRun) { Write-Host "[clean --dry-run] Would remove: $rel" -ForegroundColor Yellow }
		else { Write-Host "[clean] Removing $rel..." -ForegroundColor Cyan; Remove-Item -LiteralPath $full -Recurse -Force }
	}
}
Write-Host '[clean] Done.' -ForegroundColor Green
