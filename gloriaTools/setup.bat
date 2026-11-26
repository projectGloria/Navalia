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
# Description: Verify required toolchains and install missing project dependencies
$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent $env:GLORIA_TOOL_DIR.TrimEnd('\', '/')
Set-Location -LiteralPath $RepoRoot

$missing = @()
$tools = @(@{ Name = 'cargo'; Args = @('--version'); Required = $true }, @{ Name = 'rustc'; Args = @('--version'); Required = $true }, @{ Name = 'gh'; Args = @('--version'); Required = $false })
Write-Host '=== Toolchain Check ===' -ForegroundColor Cyan
foreach ($t in $tools) {
	$cmd = Get-Command $t.Name -ErrorAction SilentlyContinue
	if (-not $cmd) {
		$tag = if ($t.Required) { 'MISSING (required)' } else { 'missing (optional)' }
		Write-Host ("  {0,-8} : {1}" -f $t.Name, $tag) -ForegroundColor $(if ($t.Required) { 'Red' } else { 'Yellow' })
		if ($t.Required) { $missing += $t.Name }
	} else {
		$ver = (& $t.Name @($t.Args) 2>$null | Select-Object -First 1)
		Write-Host ("  {0,-8} : {1}" -f $t.Name, $ver) -ForegroundColor Green
	}
}
$wv2Keys = @('HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}','HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}','HKCU:\Software\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}')
$wv2Ver = $null
foreach ($k in $wv2Keys) { if (Test-Path $k) { $pv = (Get-ItemProperty -Path $k -ErrorAction SilentlyContinue).pv; if ($pv -and $pv -ne '0.0.0.0') { $wv2Ver = $pv; break } } }
if ($wv2Ver) { Write-Host ("  {0,-8} : {1}" -f 'WebView2', $wv2Ver) -ForegroundColor Green } else { Write-Host '  WebView2 : MISSING' -ForegroundColor Red; $missing += 'WebView2' }
if ($missing.Count -gt 0) { throw "Missing required prerequisites: $($missing -join ', ')" }
Write-Host '[setup] Environment is ready.' -ForegroundColor Green
