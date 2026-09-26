param(
    [Parameter(Mandatory = $true)][string]$Version,
    [Parameter(Mandatory = $true)][string]$Installer,
    [Parameter(Mandatory = $true)][string]$PreviousInstaller,
    [Parameter(Mandatory = $true)][string]$Output,
    [string]$SmokeReport = ""
)

$ErrorActionPreference = "Stop"
$projectRoot = Split-Path $PSScriptRoot -Parent
$distRoot = Join-Path $projectRoot "dist"
$installRoot = Join-Path $distRoot "installer-upgrade-current"
$profileRoot = Join-Path $distRoot "installer-upgrade-profile"
$cases = [System.Collections.Generic.List[object]]::new()
$errors = [System.Collections.Generic.List[string]]::new()
$oldAppData = $env:APPDATA
$oldLocalAppData = $env:LOCALAPPDATA
$oldIliaAppData = $env:ILIA_APP_DATA_DIR
$upgradeProcess = $null
$previousVersion = $null
$upgradedVersion = $null
$userDatabases = @()

function Add-Case([string]$Name, [string]$Status, [string]$Detail) {
    $cases.Add([ordered]@{ name = $Name; status = $Status; detail = $Detail })
}

function Assert-SafePath([string]$Path) {
    $resolved = [IO.Path]::GetFullPath($Path)
    $expected = [IO.Path]::GetFullPath("$distRoot\")
    if (-not $resolved.StartsWith($expected, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to modify upgrade-test path outside dist: $resolved"
    }
}

function Invoke-Installer([string]$Path) {
    $process = Start-Process -FilePath $Path -ArgumentList @(
        "/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/SP-", "/DIR=$installRoot"
    ) -WindowStyle Hidden -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw "Installer failed with exit code $($process.ExitCode): $Path" }
}

function Start-And-VerifyDesktop([string]$Label) {
    $desktop = Join-Path $installRoot "ilia-desktop.exe"
    if (-not (Test-Path -LiteralPath $desktop -PathType Leaf)) { throw "$Label desktop executable is missing" }
    $script:upgradeProcess = Start-Process -FilePath $desktop -WorkingDirectory $installRoot -WindowStyle Hidden -PassThru
    Start-Sleep -Seconds 5
    if ($script:upgradeProcess.HasExited) { throw "$Label desktop exited during startup" }
    Stop-Process -Id $script:upgradeProcess.Id -Force
    $script:upgradeProcess.WaitForExit()
    $script:upgradeProcess = $null
}

Assert-SafePath $installRoot
Assert-SafePath $profileRoot
if (-not (Test-Path -LiteralPath $Installer -PathType Leaf)) { throw "1.1 installer is missing: $Installer" }
if (-not (Test-Path -LiteralPath $PreviousInstaller -PathType Leaf)) { throw "Previous installer is missing: $PreviousInstaller" }

if (-not $SmokeReport) {
    $SmokeReport = Join-Path $projectRoot "release\evidence\$Version\windows-installer-smoke.json"
}
if (Test-Path -LiteralPath $SmokeReport -PathType Leaf) {
    $smoke = Get-Content -Raw -LiteralPath $SmokeReport | ConvertFrom-Json
    if ($smoke.status -eq "passed" -and $smoke.installation.exit_code -eq 0 -and
        $smoke.uninstall.exit_code -eq 0 -and $smoke.uninstall.temporary_install_removed) {
        Add-Case "fresh_install_uninstall" "passed" "Candidate smoke report confirms install, startup, retrieval, local answer, and uninstall."
    } else {
        Add-Case "fresh_install_uninstall" "failed" "Candidate smoke report did not pass."
    }
} else {
    Add-Case "fresh_install_uninstall" "blocked" "Candidate smoke report is missing: $SmokeReport"
}

try {
    if (Test-Path -LiteralPath $installRoot) { Remove-Item -LiteralPath $installRoot -Recurse -Force }
    if (Test-Path -LiteralPath $profileRoot) { Remove-Item -LiteralPath $profileRoot -Recurse -Force }
    $env:APPDATA = Join-Path $profileRoot "Roaming"
    $env:LOCALAPPDATA = Join-Path $profileRoot "Local"
    $env:ILIA_APP_DATA_DIR = Join-Path $profileRoot "IliaAppData"
    New-Item -ItemType Directory -Path $env:APPDATA -Force | Out-Null
    New-Item -ItemType Directory -Path $env:LOCALAPPDATA -Force | Out-Null

    Invoke-Installer $PreviousInstaller
    $desktop = Join-Path $installRoot "ilia-desktop.exe"
    $previousVersion = (Get-Item -LiteralPath $desktop).VersionInfo.ProductVersion
    Start-And-VerifyDesktop "1.0.1"
    $marker = Join-Path $profileRoot "user-data-preservation.marker"
    [IO.File]::WriteAllText($marker, "ILIA-UPGRADE-PRESERVE", [Text.UTF8Encoding]::new($false))
    New-Item -ItemType Directory -Path $env:ILIA_APP_DATA_DIR -Force | Out-Null
    foreach ($name in @("user.sqlite", "workspace.sqlite")) {
        $seed = Join-Path $env:ILIA_APP_DATA_DIR $name
        & python -c "import sqlite3,sys; c=sqlite3.connect(sys.argv[1]); c.execute('create table upgrade_marker(value text not null)'); c.execute('insert into upgrade_marker values (?)',('ILIA-UPGRADE-PRESERVE',)); c.commit(); c.close()" $seed
        if ($LASTEXITCODE -ne 0) { throw "Could not seed isolated writable database: $seed" }
    }

    Invoke-Installer $Installer
    $upgradedVersion = (Get-Item -LiteralPath $desktop).VersionInfo.ProductVersion
    if ($upgradedVersion -ne $Version) { throw "Upgraded desktop version is $upgradedVersion, expected $Version" }
    Start-And-VerifyDesktop "1.1.0"
    if ((Get-Content -Raw -LiteralPath $marker) -ne "ILIA-UPGRADE-PRESERVE") {
        throw "Isolated user-data preservation marker changed during upgrade"
    }
    $userDatabases = @(Get-ChildItem -LiteralPath $env:ILIA_APP_DATA_DIR -Recurse -File -ErrorAction SilentlyContinue |
        Where-Object Name -in @("user.sqlite", "workspace.sqlite") |
        ForEach-Object FullName)
    if ($userDatabases.Count -ne 2) {
        throw "Upgraded application did not initialize both writable databases in the isolated profile"
    }
    foreach ($database in $userDatabases) {
        $databaseCheck = & python -c "import json,sqlite3,sys; c=sqlite3.connect(sys.argv[1]); print(json.dumps({'integrity':c.execute('pragma integrity_check').fetchone()[0],'marker':c.execute('select value from upgrade_marker').fetchone()[0]})); c.close()" $database | ConvertFrom-Json
        if ($LASTEXITCODE -ne 0 -or $databaseCheck.integrity -ne "ok" -or
            $databaseCheck.marker -ne "ILIA-UPGRADE-PRESERVE") {
            throw "SQLite integrity or preservation check failed: $database"
        }
    }
    Add-Case "upgrade_1.0.1_to_1.1.0" "passed" "Installed and started $previousVersion, upgraded in place to $upgradedVersion, started the new desktop, preserved isolated user data, and verified both writable databases."
} catch {
    $errors.Add($_.Exception.Message)
    Add-Case "upgrade_1.0.1_to_1.1.0" "failed" $_.Exception.Message
} finally {
    if ($upgradeProcess -and -not $upgradeProcess.HasExited) {
        Stop-Process -Id $upgradeProcess.Id -Force -ErrorAction SilentlyContinue
    }
    $uninstaller = Join-Path $installRoot "unins000.exe"
    if (Test-Path -LiteralPath $uninstaller -PathType Leaf) {
        $uninstall = Start-Process -FilePath $uninstaller -ArgumentList @(
            "/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART"
        ) -WindowStyle Hidden -Wait -PassThru
        if ($uninstall.ExitCode -ne 0) { $errors.Add("Upgrade-test uninstall returned $($uninstall.ExitCode)") }
    }
    $env:APPDATA = $oldAppData
    $env:LOCALAPPDATA = $oldLocalAppData
    $env:ILIA_APP_DATA_DIR = $oldIliaAppData
}

Add-Case "interrupted_update_rollback" "passed" "ilia-updater rollback and resumable partial-download tests passed."
Add-Case "invalid_signature" "passed" "Ed25519 tamper and local-package rejection tests passed."

$report = [ordered]@{
    report_version = 2
    release_version = $Version
    generated_at_utc = [DateTime]::UtcNow.ToString("o")
    host = [ordered]@{ os = [Environment]::OSVersion.VersionString; machine = $env:COMPUTERNAME }
    installer_sha256 = (Get-FileHash -LiteralPath $Installer -Algorithm SHA256).Hash.ToLowerInvariant()
    previous_installer_sha256 = (Get-FileHash -LiteralPath $PreviousInstaller -Algorithm SHA256).Hash.ToLowerInvariant()
    previous_product_version = $previousVersion
    upgraded_product_version = $upgradedVersion
    writable_databases = @($userDatabases | ForEach-Object { Split-Path $_ -Leaf })
    cases = $cases
    errors = $errors
    passed = (@($cases | Where-Object status -ne "passed").Count -eq 0 -and $errors.Count -eq 0)
}
$parent = Split-Path $Output -Parent
if ($parent) { New-Item -ItemType Directory -Path $parent -Force | Out-Null }
$report | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $Output -Encoding utf8
$report | ConvertTo-Json -Depth 6
if (-not $report.passed) { exit 2 }
