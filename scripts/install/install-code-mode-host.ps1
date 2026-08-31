<#
.SYNOPSIS
Repairs the missing Code Mode host in a Windows Alpha-5 single-binary install.

.DESCRIPTION
Locates the codex-code-mode-host.exe shipped with the compatible official
@openai/codex 0.147.0 npm package, verifies its SHA-256 digest, and installs it
beside codex-extended-workflow.exe. The script never overwrites a different
existing host.

This is a repair for an installation that copied only the Rust CLI binary.
Future codex-extended-workflow package releases should ship the CLI and host
together and therefore do not need this script.

.EXAMPLE
.\scripts\install\install-code-mode-host.ps1 -WhatIf

Validates discovery, versions, and hashes without changing files.

.EXAMPLE
.\scripts\install\install-code-mode-host.ps1

Installs the verified host or verifies an already matching installation.
#>
[CmdletBinding(SupportsShouldProcess = $true, ConfirmImpact = "Medium")]
param(
    [string]$SourceHostPath,
    [string]$ExtendedCliPath,
    [ValidatePattern("^[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?$")]
    [string]$ExpectedCodexVersion = "0.147.0",
    [ValidatePattern("^[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?$")]
    [string]$ExpectedExtendedWorkflowVersion = "0.147.1-alpha-5",
    [ValidatePattern("^[0-9A-Fa-f]{64}$")]
    [string]$ExpectedSha256 = "37c23a542037e1bcfd0fa7eb4a150c697229d7ff31bf675c519d5bff7226b191"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Write-Step {
    param(
        [string]$Message
    )

    Write-Host "==> $Message"
}

function Resolve-CommandPath {
    param(
        [string]$Name
    )

    $command = Get-Command $Name -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($null -eq $command -or [string]::IsNullOrWhiteSpace($command.Source)) {
        throw "Could not find '$Name' in PATH."
    }

    return [System.IO.Path]::GetFullPath($command.Source)
}

function Get-ExecutableVersion {
    param(
        [string]$Path
    )

    $output = @(& $Path --version 2>&1)
    if ($LASTEXITCODE -ne 0) {
        throw "Could not read the version from '$Path': $($output -join [Environment]::NewLine)"
    }

    return ($output -join " ").Trim()
}

function Get-NpmGlobalRoots {
    $roots = [System.Collections.Generic.List[string]]::new()
    $npmCommand = Get-Command npm.cmd -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($null -eq $npmCommand) {
        $npmCommand = Get-Command npm -ErrorAction SilentlyContinue | Select-Object -First 1
    }

    if ($null -ne $npmCommand) {
        $npmRootOutput = @(& $npmCommand.Source root --global 2>$null)
        if ($LASTEXITCODE -eq 0) {
            foreach ($line in $npmRootOutput) {
                $candidate = ([string]$line).Trim()
                if (-not [string]::IsNullOrWhiteSpace($candidate)) {
                    $roots.Add([System.IO.Path]::GetFullPath($candidate))
                }
            }
        }
    }

    $codexCommand = Get-Command codex -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($null -ne $codexCommand -and -not [string]::IsNullOrWhiteSpace($codexCommand.Source)) {
        $commandDirectory = Split-Path -Parent $codexCommand.Source
        $roots.Add([System.IO.Path]::GetFullPath((Join-Path $commandDirectory "node_modules")))
    }

    return @($roots | Select-Object -Unique)
}

function Resolve-NpmHostPath {
    param(
        [string]$ExpectedVersion
    )

    foreach ($npmRoot in Get-NpmGlobalRoots) {
        $packageRoot = Join-Path $npmRoot "@openai\codex"
        $packageJsonPath = Join-Path $packageRoot "package.json"
        $candidate = Join-Path $packageRoot "node_modules\@openai\codex-win32-x64\vendor\x86_64-pc-windows-msvc\bin\codex-code-mode-host.exe"
        if (-not (Test-Path -LiteralPath $packageJsonPath -PathType Leaf) -or
            -not (Test-Path -LiteralPath $candidate -PathType Leaf)) {
            continue
        }

        $package = Get-Content -LiteralPath $packageJsonPath -Raw | ConvertFrom-Json
        if ([string]$package.version -ne $ExpectedVersion) {
            continue
        }

        return [System.IO.Path]::GetFullPath($candidate)
    }

    throw "Could not find the codex-code-mode-host from @openai/codex $ExpectedVersion. Install that exact npm version or pass -SourceHostPath."
}

if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
    throw "This repair script supports Windows only."
}

if ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne
    [System.Runtime.InteropServices.Architecture]::X64) {
    throw "This repair script currently supports Windows x64 only."
}

if ([string]::IsNullOrWhiteSpace($ExtendedCliPath)) {
    $ExtendedCliPath = Resolve-CommandPath -Name "codex-extended-workflow"
} else {
    $ExtendedCliPath = [System.IO.Path]::GetFullPath($ExtendedCliPath)
}

if (-not (Test-Path -LiteralPath $ExtendedCliPath -PathType Leaf)) {
    throw "Could not find codex-extended-workflow at '$ExtendedCliPath'."
}

$expectedExtendedOutput = "codex-extended-workflow v$ExpectedExtendedWorkflowVersion"
$extendedOutput = Get-ExecutableVersion -Path $ExtendedCliPath
if ($extendedOutput -ne $expectedExtendedOutput) {
    throw "Expected '$expectedExtendedOutput' but found '$extendedOutput'. Refusing to install a potentially incompatible host."
}

if ([string]::IsNullOrWhiteSpace($SourceHostPath)) {
    $SourceHostPath = Resolve-NpmHostPath -ExpectedVersion $ExpectedCodexVersion
} else {
    $SourceHostPath = [System.IO.Path]::GetFullPath($SourceHostPath)
}

if (-not (Test-Path -LiteralPath $SourceHostPath -PathType Leaf)) {
    throw "Could not find the source host at '$SourceHostPath'."
}

$expectedHash = $ExpectedSha256.ToLowerInvariant()
$sourceHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $SourceHostPath).Hash.ToLowerInvariant()
if ($sourceHash -ne $expectedHash) {
    throw "Source host SHA-256 mismatch. Expected '$expectedHash' but found '$sourceHash'."
}

$targetDirectory = Split-Path -Parent $ExtendedCliPath
$targetHostPath = Join-Path $targetDirectory "codex-code-mode-host.exe"
Write-Step "Verified source host: $SourceHostPath"
Write-Step "Verified extended CLI: $extendedOutput"

if (Test-Path -LiteralPath $targetHostPath) {
    $targetHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $targetHostPath).Hash.ToLowerInvariant()
    if ($targetHash -ne $sourceHash) {
        throw "A different host already exists at '$targetHostPath'. It was not overwritten."
    }

    Write-Step "The matching code-mode host is already installed: $targetHostPath"
} else {
    if (-not $PSCmdlet.ShouldProcess($targetHostPath, "Install the verified codex-code-mode-host")) {
        Write-Step "Preview complete; no files were changed."
        return
    }

    $temporaryHostPath = Join-Path $targetDirectory (".codex-code-mode-host.{0}.tmp" -f [Guid]::NewGuid().ToString("N"))
    try {
        Copy-Item -LiteralPath $SourceHostPath -Destination $temporaryHostPath
        $temporaryHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $temporaryHostPath).Hash.ToLowerInvariant()
        if ($temporaryHash -ne $sourceHash) {
            throw "Temporary host SHA-256 mismatch after copying."
        }

        Move-Item -LiteralPath $temporaryHostPath -Destination $targetHostPath
    } finally {
        if (Test-Path -LiteralPath $temporaryHostPath) {
            Remove-Item -LiteralPath $temporaryHostPath -Force
        }
    }
}

$installedHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $targetHostPath).Hash.ToLowerInvariant()
if ($installedHash -ne $sourceHash) {
    throw "Installed host SHA-256 mismatch."
}

$helpOutput = @(& $targetHostPath --help 2>&1)
if ($LASTEXITCODE -ne 0 -or ($helpOutput -join [Environment]::NewLine) -notmatch "Usage: codex-code-mode-host") {
    throw "The installed code-mode host did not pass its --help smoke test."
}

Write-Step "PASS: code-mode host installed and verified: $targetHostPath"
Write-Host "Restart every running codex-extended-workflow session before using Code Mode."
