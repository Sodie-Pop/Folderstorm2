<#
.SYNOPSIS
    Puts the built viewer, the MCP helper and the runtime files in a folder
    ready to zip. Folderstorm2's counterpart of upstream's scripts/package.ps1,
    which cannot be used as is: it expects `--version` to print "Aurora Viewer
    <version>", and Folderstorm2 prints its own name. Same files, same
    aurora-install.json. Used by .github/workflows/folderstorm-release.yml.

.DESCRIPTION
    On Windows the viewer is copied under its shipped name (EXE of
    crates/folderstorm-brand/src/lib.rs, "Folderstorm2.exe"): the updater
    looks for that name, in the installation and in the package. Without
    -Out, the folder is named as the updater expects the package of this
    version: "<RELEASE_PREFIX>-v<version>-windows-x64" ("-linux-x64" for an
    executable without extension). Zipped with its folder, it is the release
    archive "<folder>.zip".

    The script writes the folder's name as its output.

.EXAMPLE
    ./folderstorm/scripts/package-release.ps1 -Exe target/release/aurora-viewer.exe -Mcp target/release/folderstorm-mcp.exe
.EXAMPLE
    ./folderstorm/scripts/package-release.ps1 -Exe target/release/aurora-viewer.exe -Mcp target/release/folderstorm-mcp.exe -Out folderstorm2-test-windows-x64
#>
param(
    [Parameter(Mandatory)][string]$Exe,
    [Parameter(Mandatory)][string]$Mcp,
    # default: the package folder of this version, named as the updater expects
    [string]$Out,
    [string]$Source = (Split-Path (Split-Path $PSScriptRoot -Parent) -Parent)
)
$ErrorActionPreference = 'Stop'

# The shipped names come from folderstorm-brand, not from this file.
$brand = Get-Content -Raw -LiteralPath (Join-Path $Source 'crates/folderstorm-brand/src/lib.rs')
function Get-BrandConstant([string]$Name) {
    if ($brand -notmatch "(?m)^pub const ${Name}: &str = `"([^`"]+)`";") { throw "folderstorm-brand has no $Name" }
    $Matches[1]
}
$exeName = Get-BrandConstant 'EXE'
$mcpName = Get-BrandConstant 'MCP_EXE'
$prefix = Get-BrandConstant 'RELEASE_PREFIX'

$windows = [bool][IO.Path]::GetExtension($Exe)
if ($windows -and (Split-Path -Leaf $Mcp) -ne $mcpName) { throw "The MCP helper must be named $mcpName, not $(Split-Path -Leaf $Mcp)" }

# "<product> <version>" -> the version (the last word)
$line = (& (Resolve-Path -LiteralPath $Exe).Path --version | Out-String).Trim()
$version = ($line -split '\s+')[-1]
if ($LASTEXITCODE -ne 0 -or $version -notmatch '^\d+\.\d+\.\d+([+-][0-9A-Za-z.+-]+)?$') { throw "Invalid package version: '$line'" }
if (-not $Out) {
    $Out = "$prefix-v$version-$(if ($windows) { 'windows' } else { 'linux' })-x64"
}
if (Test-Path -LiteralPath $Out) { throw "$Out already exists: remove it first" }

New-Item -ItemType Directory -Force -Path (Join-Path $Out 'assets/emoji') | Out-Null
if ($windows) {
    # the shipped name (release plan, section 1); cargo builds aurora-viewer.exe
    Copy-Item -LiteralPath $Exe -Destination (Join-Path $Out $exeName)
} else {
    Copy-Item -LiteralPath $Exe -Destination $Out
}
Copy-Item -LiteralPath $Mcp -Destination $Out
# Licences of what is compiled into the exe (emoji font, spelling dictionaries)
# and the assets folder the updater requires, as upstream's package.ps1.
Copy-Item -LiteralPath (Join-Path $Source 'assets/emoji/LICENSE-OFL.txt') -Destination (Join-Path $Out 'assets/emoji')
$dictionaries = Join-Path $Source 'crates/aurora-viewer/assets/dictionaries'
$dictionariesOut = Join-Path $Out 'assets/dictionaries'
New-Item -ItemType Directory -Force -Path $dictionariesOut | Out-Null
Copy-Item -LiteralPath (Join-Path $dictionaries 'README.md') -Destination $dictionariesOut
Get-ChildItem -LiteralPath $dictionaries -Directory | ForEach-Object {
    $license = Join-Path $_.FullName 'LICENSE.txt'
    if (Test-Path -LiteralPath $license) {
        Copy-Item -LiteralPath $license -Destination (Join-Path $dictionariesOut "$($_.Name)-LICENSE.txt")
    }
}
Copy-Item -LiteralPath ('LICENSE', 'NOTICE.md', 'README.md' | ForEach-Object { Join-Path $Source $_ }) -Destination $Out
if (-not $windows -and -not $IsMacOS) {
    # Linux: desktop entry and icon, as upstream's package.ps1
    Copy-Item -LiteralPath (Join-Path $Source 'packaging/linux/aurora-viewer.desktop') -Destination $Out
    Copy-Item -LiteralPath (Join-Path $Source 'assets/branding/aurora-logo.svg') -Destination (Join-Path $Out 'aurora-viewer.svg')
}
$json = @{ schema = 1; version = $version } | ConvertTo-Json
[IO.File]::WriteAllText((Join-Path (Resolve-Path -LiteralPath $Out).Path 'aurora-install.json'), $json, [Text.UTF8Encoding]::new($false))
Write-Output $Out
