<#
.SYNOPSIS
Installs a verified codex-extended-workflow Windows release package.

.DESCRIPTION
Downloads (or accepts local paths to) the canonical Windows package and its
SHA-256 manifest, validates the complete package contract, runs CLI and Code
Mode host smoke tests, and only then replaces the managed installation.

.EXAMPLE
.\install-extended-workflow.ps1 -Release v0.147.1-alpha-6

.EXAMPLE
.\install-extended-workflow.ps1 -ArchivePath .\package.zip -ChecksumPath .\SHA256SUMS -WhatIf
#>
#Requires -Version 5.1
[CmdletBinding(SupportsShouldProcess = $true, ConfirmImpact = "Medium")]
param(
    [string]$Release = "latest",
    [ValidatePattern("^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$")]
    [string]$Repository = "mikmann/own-codex",
    [string]$ArchivePath,
    [string]$ChecksumPath,
    [string]$InstallRoot = (Join-Path $env:LOCALAPPDATA "Programs\codex-extended-workflow"),
    [switch]$NoPathUpdate
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

function Write-Step {
    param([string]$Message)
    Write-Host "==> $Message"
}

function New-GitHubHeaders {
    param([string]$Accept = "application/vnd.github+json")

    $headers = @{
        Accept = $Accept
        "User-Agent" = "codex-extended-workflow-installer"
        "X-GitHub-Api-Version" = "2022-11-28"
    }
    $token = if (-not [string]::IsNullOrWhiteSpace($env:GH_TOKEN)) {
        $env:GH_TOKEN
    } else {
        $env:GITHUB_TOKEN
    }
    if (-not [string]::IsNullOrWhiteSpace($token)) {
        $headers.Authorization = "Bearer $token"
    }
    return $headers
}

function Get-ReleaseMetadata {
    if ($Release -eq "latest") {
        $releases = @(Invoke-RestMethod -Uri "https://api.github.com/repos/$Repository/releases?per_page=20" -Headers (New-GitHubHeaders))
        $metadata = $releases | Where-Object { -not $_.draft } | Select-Object -First 1
        if ($null -eq $metadata) {
            throw "No published release was found for $Repository."
        }
        return $metadata
    }

    $tag = if ($Release.StartsWith("v")) { $Release } else { "v$Release" }
    $encodedTag = [Uri]::EscapeDataString($tag)
    return Invoke-RestMethod -Uri "https://api.github.com/repos/$Repository/releases/tags/$encodedTag" -Headers (New-GitHubHeaders)
}

function Save-ReleaseAsset {
    param(
        [object]$Metadata,
        [string]$Name,
        [string]$Destination
    )

    $asset = @($Metadata.assets) | Where-Object { $_.name -ceq $Name }
    if ($asset.Count -ne 1) {
        throw "Release '$($Metadata.tag_name)' must contain exactly one '$Name' asset."
    }
    Invoke-WebRequest -Uri $asset[0].url -Headers (New-GitHubHeaders -Accept "application/octet-stream") -OutFile $Destination
}

function Get-ManifestHash {
    param(
        [string]$ManifestPath,
        [string]$ArchiveName
    )

    $expectedHash = $null
    foreach ($line in Get-Content -LiteralPath $ManifestPath) {
        if ($line -notmatch "^([0-9A-Fa-f]{64})\s+\*?(.+)$") {
            continue
        }
        if ($Matches[2].Trim() -cne $ArchiveName) {
            continue
        }
        if ($null -ne $expectedHash) {
            throw "Checksum manifest contains duplicate entries for '$ArchiveName'."
        }
        $expectedHash = $Matches[1].ToLowerInvariant()
    }
    if ($null -eq $expectedHash) {
        throw "Checksum manifest does not contain '$ArchiveName'."
    }
    return $expectedHash
}

function Assert-Package {
    param(
        [string]$PackageRoot,
        [string]$ExpectedTarget,
        [string]$ExpectedVersion
    )

    $metadataPath = Join-Path $PackageRoot "codex-package.json"
    if (-not (Test-Path -LiteralPath $metadataPath -PathType Leaf)) {
        throw "Package is missing codex-package.json."
    }
    $metadata = Get-Content -LiteralPath $metadataPath -Raw | ConvertFrom-Json
    $expectedEntrypoint = "bin/codex-extended-workflow.exe"
    if ($metadata.layoutVersion -ne 1 -or
        $metadata.variant -cne "codex-extended-workflow" -or
        $metadata.target -cne $ExpectedTarget -or
        $metadata.entrypoint -cne $expectedEntrypoint) {
        throw "Package metadata does not describe the required codex-extended-workflow Windows layout."
    }
    if (-not [string]::IsNullOrWhiteSpace($ExpectedVersion) -and $metadata.version -cne $ExpectedVersion) {
        throw "Package version '$($metadata.version)' does not match release version '$ExpectedVersion'."
    }

    $requiredFiles = @(
        "bin\codex-extended-workflow.exe",
        "bin\codex-code-mode-host.exe",
        "codex-path\rg.exe",
        "codex-resources\codex-command-runner.exe",
        "codex-resources\codex-windows-sandbox-setup.exe"
    )
    foreach ($relativePath in $requiredFiles) {
        if (-not (Test-Path -LiteralPath (Join-Path $PackageRoot $relativePath) -PathType Leaf)) {
            throw "Package is missing required file '$relativePath'."
        }
    }

    $cli = Join-Path $PackageRoot "bin\codex-extended-workflow.exe"
    $cliOutput = @(& $cli --version 2>&1)
    $expectedOutput = "codex-extended-workflow v$($metadata.version)"
    if ($LASTEXITCODE -ne 0 -or ($cliOutput -join " ").Trim() -cne $expectedOutput) {
        throw "Extended CLI smoke test failed; expected '$expectedOutput'."
    }

    $hostExecutable = Join-Path $PackageRoot "bin\codex-code-mode-host.exe"
    $hostOutput = @(& $hostExecutable --help 2>&1)
    if ($LASTEXITCODE -ne 0 -or ($hostOutput -join [Environment]::NewLine) -notmatch "Usage: codex-code-mode-host") {
        throw "Code Mode host smoke test failed."
    }
    return [string]$metadata.version
}

function Prepend-PathEntry {
    param(
        [string]$PathValue,
        [string]$Entry
    )

    $entries = @($PathValue -split ";" | Where-Object {
        -not [string]::IsNullOrWhiteSpace($_) -and
        -not $_.TrimEnd("\").Equals($Entry.TrimEnd("\"), [StringComparison]::OrdinalIgnoreCase)
    })
    return (@($Entry) + $entries) -join ";"
}

if ($env:OS -ne "Windows_NT") {
    throw "This installer supports Windows only."
}
if ([Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne [Runtime.InteropServices.Architecture]::X64) {
    throw "This release installer currently supports Windows x64 only."
}
if ([string]::IsNullOrWhiteSpace($InstallRoot)) {
    throw "InstallRoot must not be empty."
}
if ([string]::IsNullOrWhiteSpace($ArchivePath) -ne [string]::IsNullOrWhiteSpace($ChecksumPath)) {
    throw "ArchivePath and ChecksumPath must be supplied together."
}

$target = "x86_64-pc-windows-msvc"
$archiveName = "codex-extended-workflow-package-$target.zip"
$manifestName = "codex-extended-workflow-package_SHA256SUMS"
$temporaryRoot = Join-Path ([IO.Path]::GetTempPath()) ("codex-extended-workflow-install-" + [Guid]::NewGuid().ToString("N"))
$extractRoot = Join-Path $temporaryRoot "package"
$resolvedTag = $null
New-Item -ItemType Directory -Path $temporaryRoot | Out-Null

try {
    if ([string]::IsNullOrWhiteSpace($ArchivePath)) {
        $releaseMetadata = Get-ReleaseMetadata
        $resolvedTag = [string]$releaseMetadata.tag_name
        $ArchivePath = Join-Path $temporaryRoot $archiveName
        $ChecksumPath = Join-Path $temporaryRoot $manifestName
        Write-Step "Downloading $resolvedTag from $Repository"
        Save-ReleaseAsset -Metadata $releaseMetadata -Name $archiveName -Destination $ArchivePath
        Save-ReleaseAsset -Metadata $releaseMetadata -Name $manifestName -Destination $ChecksumPath
    } else {
        $ArchivePath = [IO.Path]::GetFullPath($ArchivePath)
        $ChecksumPath = [IO.Path]::GetFullPath($ChecksumPath)
    }

    if (-not (Test-Path -LiteralPath $ArchivePath -PathType Leaf) -or
        -not (Test-Path -LiteralPath $ChecksumPath -PathType Leaf)) {
        throw "Package archive or checksum manifest was not found."
    }
    $expectedHash = Get-ManifestHash -ManifestPath $ChecksumPath -ArchiveName ([IO.Path]::GetFileName($ArchivePath))
    $actualHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $ArchivePath).Hash.ToLowerInvariant()
    if ($actualHash -cne $expectedHash) {
        throw "Package SHA-256 mismatch. Expected '$expectedHash' but found '$actualHash'."
    }

    Expand-Archive -LiteralPath $ArchivePath -DestinationPath $extractRoot
    $expectedVersion = if ($null -ne $resolvedTag -and $resolvedTag.StartsWith("v")) {
        $resolvedTag.Substring(1)
    } else {
        ""
    }
    $version = Assert-Package -PackageRoot $extractRoot -ExpectedTarget $target -ExpectedVersion $expectedVersion
    Write-Step "Verified complete package v$version and SHA-256 $actualHash"

    $InstallRoot = [IO.Path]::GetFullPath($InstallRoot).TrimEnd("\")
    $installParent = Split-Path -Parent $InstallRoot
    if ([string]::IsNullOrWhiteSpace($installParent) -or $InstallRoot -ceq [IO.Path]::GetPathRoot($InstallRoot)) {
        throw "Refusing unsafe InstallRoot '$InstallRoot'."
    }
    if (-not $PSCmdlet.ShouldProcess($InstallRoot, "Install verified codex-extended-workflow v$version")) {
        Write-Step "Preview complete; no files were changed."
        return
    }

    New-Item -ItemType Directory -Force -Path $installParent | Out-Null
    $stagingRoot = Join-Path $installParent (".codex-extended-workflow.staging." + [Guid]::NewGuid().ToString("N"))
    $backupRoot = Join-Path $installParent (".codex-extended-workflow.backup." + [Guid]::NewGuid().ToString("N"))
    try {
        Copy-Item -LiteralPath $extractRoot -Destination $stagingRoot -Recurse
        [void](Assert-Package -PackageRoot $stagingRoot -ExpectedTarget $target -ExpectedVersion $version)
        if (Test-Path -LiteralPath $InstallRoot) {
            $installedMetadata = Join-Path $InstallRoot "codex-package.json"
            if (-not (Test-Path -LiteralPath $installedMetadata -PathType Leaf) -or
                (Get-Content -LiteralPath $installedMetadata -Raw | ConvertFrom-Json).variant -cne "codex-extended-workflow") {
                throw "Refusing to replace an installation not owned by codex-extended-workflow."
            }
            Move-Item -LiteralPath $InstallRoot -Destination $backupRoot
        }
        try {
            Move-Item -LiteralPath $stagingRoot -Destination $InstallRoot
            [void](Assert-Package -PackageRoot $InstallRoot -ExpectedTarget $target -ExpectedVersion $version)
        } catch {
            if (Test-Path -LiteralPath $InstallRoot) {
                Remove-Item -LiteralPath $InstallRoot -Recurse -Force
            }
            if (Test-Path -LiteralPath $backupRoot) {
                Move-Item -LiteralPath $backupRoot -Destination $InstallRoot
            }
            throw
        }
        if (Test-Path -LiteralPath $backupRoot) {
            Remove-Item -LiteralPath $backupRoot -Recurse -Force
        }
    } finally {
        if (Test-Path -LiteralPath $stagingRoot) {
            Remove-Item -LiteralPath $stagingRoot -Recurse -Force
        }
    }

    $binDirectory = Join-Path $InstallRoot "bin"
    if (-not $NoPathUpdate) {
        $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
        [Environment]::SetEnvironmentVariable("Path", (Prepend-PathEntry -PathValue $userPath -Entry $binDirectory), "User")
        $env:Path = Prepend-PathEntry -PathValue $env:Path -Entry $binDirectory
        Write-Step "PATH updated for this and future PowerShell sessions."
    }
    Write-Step "PASS: codex-extended-workflow v$version installed with its verified Code Mode host."
    Write-Host "Run: codex-extended-workflow"
} finally {
    if (Test-Path -LiteralPath $temporaryRoot) {
        Remove-Item -LiteralPath $temporaryRoot -Recurse -Force
    }
}
