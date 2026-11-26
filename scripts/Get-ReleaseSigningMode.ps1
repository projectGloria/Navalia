# Select a signing lane before building. Dry-run signatures never reach Releases.
[CmdletBinding()]
param(
    [switch]$DryRun,
    [Parameter(Mandatory)][string]$Version,
    [string]$Organization = '',
    [string]$Project = '',
    [string]$Policy = '',
    [string]$ProductionThumbprint = '',
    [bool]$TokenPresent = $false
)
$ErrorActionPreference = 'Stop'
if ($Version -cnotmatch '\A(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(\-[0-9A-Za-z.-]+)?\z') {
    throw 'A valid semantic workspace version is required.'
}
if (-not $Organization.Trim() -and -not $Project.Trim() -and -not $TokenPresent) {
    return [pscustomobject]@{ Mode = 'Unsigned'; Policy = ''; Version = $Version }
}
if (-not $Organization.Trim() -or -not $Project.Trim() -or -not $TokenPresent) {
    throw 'Configure SIGNPATH_ORGANIZATION_ID, SIGNPATH_PROJECT_SLUG and the SIGNPATH_API_TOKEN secret before requesting signing.'
}
if ($DryRun) {
    return [pscustomobject]@{ Mode = 'Test'; Policy = 'test-signing'; Version = $Version }
}
if ($Policy -cne 'release-signing') {
    throw 'A public release requires SIGNPATH_SIGNING_POLICY_SLUG=release-signing. Use dry_run=true for isolated test-signing; it does not create a release.'
}
if ($ProductionThumbprint -cnotmatch '\A[0-9A-Fa-f]{40}\z' -or
    $ProductionThumbprint -eq '3A93532146A10D3B1FA8DEAF4192AD4A232FFCAC') {
    throw 'Set SIGNPATH_PRODUCTION_CERTIFICATE_THUMBPRINT to the production certificate thumbprint confirmed in SignPath.'
}
[pscustomobject]@{ Mode = 'Production'; Policy = $Policy; Version = $Version }
