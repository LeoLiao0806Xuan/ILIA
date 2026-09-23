param(
    [Parameter(Mandatory = $true)][string]$ReleaseId,
    [Parameter(Mandatory = $true)][string]$BaseUrl,
    [Parameter(Mandatory = $true)][string]$ComponentsFile,
    [string]$PrivateKey = ".secrets/update-private-key.json",
    [string]$OutputRoot = "update/releases"
)

$ErrorActionPreference = "Stop"
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$componentsPath = (Resolve-Path $ComponentsFile).Path
$privateKeyPath = (Resolve-Path $PrivateKey).Path
$releaseRoot = [System.IO.Path]::GetFullPath((Join-Path $projectRoot (Join-Path $OutputRoot $ReleaseId)))
$allowedRoot = [System.IO.Path]::GetFullPath((Join-Path $projectRoot $OutputRoot))
if (-not $releaseRoot.StartsWith($allowedRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "Release output must remain under $allowedRoot"
}
if (Test-Path -LiteralPath $releaseRoot) {
    throw "Refusing to overwrite an existing release directory: $releaseRoot"
}
New-Item -ItemType Directory -Path $releaseRoot -Force | Out-Null

$sourceComponents = @(Get-Content -Raw -LiteralPath $componentsPath | ConvertFrom-Json)
if ($sourceComponents.Count -eq 0) { throw "At least one update component is required" }
$manifestComponents = @(foreach ($component in $sourceComponents) {
    $payload = (Resolve-Path $component.payload_path).Path
    $safeId = [string]$component.id
    if ($safeId -notmatch '^[A-Za-z0-9._-]+$') { throw "Unsafe component id: $safeId" }
    $payloadName = "$safeId.payload"
    Copy-Item -LiteralPath $payload -Destination (Join-Path $releaseRoot $payloadName)
    $item = Get-Item -LiteralPath $payload
    [pscustomobject][ordered]@{
        id = $safeId
        kind = [string]$component.kind
        version = [string]$component.version
        from_version = if ($null -eq $component.from_version) { $null } else { [string]$component.from_version }
        target = [string]$component.target
        payload_url = "$($BaseUrl.TrimEnd('/'))/$ReleaseId/$payloadName"
        payload_size = $item.Length
        payload_sha256 = (Get-FileHash -LiteralPath $payload -Algorithm SHA256).Hash.ToLowerInvariant()
        payload_format = [string]$component.payload_format
    }
})
$manifest = [ordered]@{
    schema_version = 1
    release_id = $ReleaseId
    channel = "stable"
    created_at = (Get-Date).ToUniversalTime().ToString("o")
    components = @($manifestComponents)
}
$manifestPath = Join-Path $releaseRoot "update-manifest.json"
$signaturePath = Join-Path $releaseRoot "update-manifest.sig"
$manifestJson = $manifest | ConvertTo-Json -Depth 8
[System.IO.File]::WriteAllText($manifestPath, $manifestJson, [System.Text.UTF8Encoding]::new($false))

. (Join-Path $projectRoot "tools\windows-toolchain.ps1")
Set-IliaGnuEnvironment
& cargo +stable-x86_64-pc-windows-gnu run --offline -p ilia-updater --bin ilia-update-sign -- sign $privateKeyPath $manifestPath $signaturePath
if ($LASTEXITCODE -ne 0) { throw "Manifest signing failed with exit code $LASTEXITCODE" }

[ordered]@{
    release_id = $ReleaseId
    output = $releaseRoot
    manifest = $manifestPath
    signature = $signaturePath
    components = $manifestComponents.Count
} | ConvertTo-Json
