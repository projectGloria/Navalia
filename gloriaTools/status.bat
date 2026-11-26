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
# Description: Show git status, version manifest sync, Brain sync, active ports, and cache sizes
$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent $env:GLORIA_TOOL_DIR.TrimEnd('\', '/')
Set-Location -LiteralPath $RepoRoot

Write-Host '=== Git ===' -ForegroundColor Cyan
$branch = (& git branch --show-current 2>$null)
$dirty = @(& git status --porcelain 2>$null)
Write-Host ("  Branch : {0}" -f $(if ($branch) { $branch } else { '<detached>' }))
Write-Host ("  Dirty  : {0} file(s)" -f $dirty.Count)

Write-Host "`n=== Brain Sync ===" -ForegroundColor Cyan
$brainBat = if (Test-Path -LiteralPath (Join-Path $RepoRoot 'install-brain.bat')) { Join-Path $RepoRoot 'install-brain.bat' } elseif (Test-Path -LiteralPath (Join-Path $env:USERPROFILE '.brain\install-brain.bat')) { Join-Path $env:USERPROFILE '.brain\install-brain.bat' } else { $null }
if ($brainBat) { & $brainBat check . } else { Write-Host '  install-brain.bat not found.' -ForegroundColor DarkGray }

Write-Host "`n=== Active Ports (3000..3050) ===" -ForegroundColor Cyan
$found = $false
if (Get-Command Get-NetTCPConnection -ErrorAction SilentlyContinue) {
	Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue | Where-Object { $_.LocalPort -ge 3000 -and $_.LocalPort -le 3050 } | ForEach-Object {
		$cim = Get-CimInstance Win32_Process -Filter "ProcessId = $($_.OwningProcess)" -ErrorAction SilentlyContinue
		Write-Host ("  Port {0} -> PID {1} ({2})" -f $_.LocalPort, $_.OwningProcess, $(if ($cim) { $cim.Name } else { '?' }))
		$found = $true
	}
}
if (-not $found) { Write-Host '  None active.' -ForegroundColor DarkGray }
