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
# Description: Interactive launcher for all scripts in gloriaTools/
$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent $env:GLORIA_TOOL_DIR.TrimEnd('\', '/')
Set-Location -LiteralPath $RepoRoot

$ToolDir = $env:GLORIA_TOOL_DIR.TrimEnd('\', '/')
while ($true) {
	$scripts = Get-ChildItem -LiteralPath $ToolDir -Filter '*.bat' -File | Where-Object { $_.Name -ine 'menu.bat' } | Sort-Object Name
	Write-Host "`n=== gloriaTools ($RepoRoot) ===" -ForegroundColor Cyan
	for ($i = 0; $i -lt $scripts.Count; $i++) {
		$raw = Get-Content -LiteralPath $scripts[$i].FullName -Encoding UTF8 -ErrorAction SilentlyContinue
		$descLine = $raw | Where-Object { $_ -match '^#\s*Description:\s*(.+)$' } | Select-Object -First 1
		$d = if ($descLine -and $descLine -match '^#\s*Description:\s*(.+)$') { $Matches[1].Trim() } else { '' }
		Write-Host ("  [{0,2}] {1,-18} {2}" -f ($i + 1), $scripts[$i].BaseName, $d)
	}
	Write-Host '  [ q] Quit'
	$choice = Read-Host "`nSelect tool"
	if (-not $choice -or $choice.Trim() -ieq 'q') { break }
	$idx = 0
	if ([int]::TryParse($choice.Trim(), [ref]$idx) -and $idx -ge 1 -and $idx -le $scripts.Count) {
		$target = $scripts[$idx - 1].FullName
		Write-Host "`n--- Running $($scripts[$idx - 1].Name) ---" -ForegroundColor Green
		& cmd.exe /c "`"$target`""
		Write-Host "--- Finished (exit $LASTEXITCODE) ---" -ForegroundColor DarkGray
	} else { Write-Host 'Invalid selection.' -ForegroundColor Yellow }
}
