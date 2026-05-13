<#
.SYNOPSIS
  Generate the Tauri updater manifest (latest.json) from signed bundle artifacts.

.DESCRIPTION
  Tauri v2 with createUpdaterArtifacts=true signs the Windows installer itself:
  - bundle/nsis/*.exe + *.exe.sig
  - bundle/msi/*.msi + *.msi.sig

  This script prefers the NSIS installer, then falls back to MSI, and writes the
  static JSON manifest consumed by @tauri-apps/plugin-updater.
#>
param(
    [Parameter(Mandatory = $true)]
    [string]$Version,

    [string]$Tag = "",

    [string]$Repository = "",

    [string]$BundleDir = "src-tauri\target\release\bundle",

    [string]$OutputPath = "latest.json",

    [string]$DownloadBaseUrl = ""
)

$ErrorActionPreference = "Stop"

if ($Version.StartsWith("v")) {
    $Version = $Version.Substring(1)
}

if (-not $Tag) {
    $Tag = "v$Version"
}

if (-not (Test-Path $BundleDir)) {
    throw "Bundle directory not found: $BundleDir"
}

function Find-SignedArtifact {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Directory,

        [Parameter(Mandatory = $true)]
        [string]$Pattern,

        [Parameter(Mandatory = $true)]
        [string]$Version
    )

    if (-not (Test-Path $Directory)) {
        return $null
    }

    $candidates = Get-ChildItem -Path $Directory -Filter $Pattern -File -ErrorAction SilentlyContinue
    if (-not $candidates) {
        return $null
    }

    $versionMatches = $candidates | Where-Object { $_.Name -like "*$Version*" }
    if ($versionMatches) {
        $candidates = $versionMatches
    }

    $artifact = $candidates | Sort-Object LastWriteTimeUtc -Descending | Select-Object -First 1
    $signaturePath = "$($artifact.FullName).sig"
    if (-not (Test-Path $signaturePath)) {
        return $null
    }

    return [ordered]@{
        Artifact = $artifact
        SignaturePath = $signaturePath
    }
}

$artifactInfo = Find-SignedArtifact `
    -Directory (Join-Path $BundleDir "nsis") `
    -Pattern "*.exe" `
    -Version $Version

if (-not $artifactInfo) {
    $artifactInfo = Find-SignedArtifact `
        -Directory (Join-Path $BundleDir "msi") `
        -Pattern "*.msi" `
        -Version $Version
}

if (-not $artifactInfo) {
    throw "No signed Windows updater artifact found. Expected bundle\nsis\*.exe.sig or bundle\msi\*.msi.sig. Check TAURI_SIGNING_PRIVATE_KEY and createUpdaterArtifacts."
}

$artifact = $artifactInfo.Artifact
$signature = (Get-Content $artifactInfo.SignaturePath -Raw).Trim()

if (-not $DownloadBaseUrl) {
    if (-not $Repository) {
        throw "Repository or DownloadBaseUrl is required to build the updater download URL."
    }
    $DownloadBaseUrl = "https://github.com/$Repository/releases/download/$Tag"
}

$DownloadBaseUrl = $DownloadBaseUrl.TrimEnd("/")
$pubDate = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")

$manifest = [ordered]@{
    version = $Version
    notes = if ($Repository) { "https://github.com/$Repository/releases/tag/$Tag" } else { "" }
    pub_date = $pubDate
    platforms = [ordered]@{
        "windows-x86_64" = [ordered]@{
            signature = $signature
            url = "$DownloadBaseUrl/$($artifact.Name)"
        }
    }
}

$json = $manifest | ConvertTo-Json -Depth 5
$utf8NoBom = New-Object System.Text.UTF8Encoding $false
[System.IO.File]::WriteAllText([System.IO.Path]::GetFullPath($OutputPath), "$json$([Environment]::NewLine)", $utf8NoBom)
Write-Host "Generated $OutputPath for $Version -> $($artifact.Name)"
