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
# Description: Stop lingering repo binaries and dev server processes (--port N)
$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent $env:GLORIA_TOOL_DIR.TrimEnd('\', '/')
Set-Location -LiteralPath $RepoRoot

$ports = 3000..3050
for ($i = 0; $i -lt $args.Count; $i++) {
	$a = [string]$args[$i]
	if ($a -eq '--port' -and ($i + 1) -lt $args.Count) { $parsed = 0; if ([int]::TryParse([string]$args[$i + 1], [ref]$parsed)) { $ports = @($parsed) }; $i++ }
	elseif ($a -like '--port=*') { $parsed = 0; if ([int]::TryParse($a.Substring(7), [ref]$parsed)) { $ports = @($parsed) } }
}
$stopped = 0
Get-Process -Name 'navalia' -ErrorAction SilentlyContinue | ForEach-Object {
	$procPath = $null; try { $procPath = $_.Path } catch {}
	if ($procPath -and $procPath.StartsWith($RepoRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
		Write-Host "[stop] Stopping navalia.exe (PID $($_.Id))..." -ForegroundColor Yellow
		Stop-Process -Id $_.Id -Force -ErrorAction SilentlyContinue
		$stopped++
	}
}
$pidsToCheck = New-Object System.Collections.Generic.HashSet[int]
if (Get-Command Get-NetTCPConnection -ErrorAction SilentlyContinue) {
	Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue | Where-Object { $ports -contains $_.LocalPort } | ForEach-Object { [void]$pidsToCheck.Add([int]$_.OwningProcess) }
}
foreach ($procId in $pidsToCheck) {
	if ($procId -le 4) { continue }
	$cim = Get-CimInstance Win32_Process -Filter "ProcessId = $procId" -ErrorAction SilentlyContinue
	if (-not $cim) { continue }
	$cmd = [string]$cim.CommandLine
	if ($cmd -and ($cmd.IndexOf($RepoRoot, [System.StringComparison]::OrdinalIgnoreCase) -ge 0 -or $cmd -match '(?i)gloria-tauri-dev-')) {
		Write-Host "[stop] Stopping $($cim.Name) (PID $procId)..." -ForegroundColor Yellow
		Stop-Process -Id $procId -Force -ErrorAction SilentlyContinue
		$stopped++
	}
}
if ($stopped -eq 0) { Write-Host '[stop] No active repo processes found.' -ForegroundColor DarkGray } else { Write-Host "[stop] Stopped $stopped process(es)." -ForegroundColor Green }
