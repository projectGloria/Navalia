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
# Description: Run pre-flight build, tests, lint, version manifest sync, and repo checks
$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent $env:GLORIA_TOOL_DIR.TrimEnd('\', '/')
Set-Location -LiteralPath $RepoRoot

$results = @()
function Invoke-Step([string]$Name, [scriptblock]$Action) {
	Write-Host "`n=== [$Name] ===" -ForegroundColor Cyan
	$ok = $false
	try {
		$global:LASTEXITCODE = 0
		& $Action
		if ($LASTEXITCODE -eq 0) { $ok = $true }
	} catch {
		Write-Host $_ -ForegroundColor Red
		$ok = $false
	}
	$script:results += [PSCustomObject]@{ Name = $Name; Passed = $ok }
}

Invoke-Step 'Rust tests (cargo test)' { & cargo test --manifest-path Cargo.toml }

Invoke-Step 'Rust lint (cargo clippy)' { & cargo clippy --manifest-path Cargo.toml --all-targets -- -D warnings }

Invoke-Step 'Version manifest sync' {
	$versions = [ordered]@{}
	if (Test-Path -LiteralPath 'package.json') { $versions['package.json'] = (Get-Content -Raw -LiteralPath 'package.json' -Encoding UTF8 | ConvertFrom-Json).version }
	if (Test-Path -LiteralPath 'package-lock.json') {
		$raw = Get-Content -Raw -LiteralPath 'package-lock.json' -Encoding UTF8
		$versions['package-lock.json'] = if ($raw -match '"version"\s*:\s*"([^"]+)"') { $Matches[1] } else { '<missing>' }
	}
	foreach ($tc in @('src-tauri/tauri.conf.json', 'tauri.conf.json')) {
		if (Test-Path -LiteralPath $tc) { $versions[$tc] = (Get-Content -Raw -LiteralPath $tc -Encoding UTF8 | ConvertFrom-Json).version }
	}
	foreach ($ct in @('src-tauri/Cargo.toml', 'Cargo.toml')) {
		if (Test-Path -LiteralPath $ct) {
			$raw = Get-Content -Raw -LiteralPath $ct -Encoding UTF8
			$versions[$ct] = if ($raw -match '(?ms)^\[(?:workspace\.)?package\].*?^version\s*=\s*"([^"]+)"') { $Matches[1] } else { '<missing>' }
		}
	}
	if (Test-Path -LiteralPath 'gloria.toml') {
		$raw = Get-Content -Raw -LiteralPath 'gloria.toml' -Encoding UTF8
		$versions['gloria.toml'] = if ($raw -match '(?m)^version\s*=\s*"([^"]+)"') { $Matches[1] } else { '<missing>' }
	}
	foreach ($k in $versions.Keys) { Write-Host ("  {0,-28} {1}" -f $k, $versions[$k]) }
	$uniq = @($versions.Values | Select-Object -Unique)
	if ($uniq.Count -ne 1 -or $uniq[0] -eq '<missing>') { throw 'Version mismatch across manifests!' }
}

Invoke-Step 'GloriaSetup manifest check' { & npx --yes gloria-setup@latest --check --manifest gloria.toml }

$brainBat = if (Test-Path -LiteralPath (Join-Path $RepoRoot 'install-brain.bat')) { Join-Path $RepoRoot 'install-brain.bat' } elseif (Test-Path -LiteralPath (Join-Path $env:USERPROFILE '.brain\install-brain.bat')) { Join-Path $env:USERPROFILE '.brain\install-brain.bat' } else { $null }
if ($brainBat) { Invoke-Step 'Brain sync check' { & $brainBat check . } }

Write-Host "`n=== Summary ===" -ForegroundColor Cyan
$failed = 0
foreach ($r in $results) {
	if ($r.Passed) { Write-Host ("  [PASS] {0}" -f $r.Name) -ForegroundColor Green }
	else { Write-Host ("  [FAIL] {0}" -f $r.Name) -ForegroundColor Red; $failed++ }
}
if ($failed -gt 0) { exit 1 }
exit 0
