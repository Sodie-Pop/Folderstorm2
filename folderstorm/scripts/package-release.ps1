<#
.SYNOPSIS
    Puts the built viewer, the MCP helper and the runtime files in a folder
    ready to zip. Folderstorm2's counterpart of upstream's scripts/package.ps1,
    which cannot be used as is: it expects `--version` to print "Aurora Viewer
    <version>", and Folderstorm2 prints its own name. Same files, same
    aurora-install.json. Used by .github/workflows/folderstorm-release.yml.

.EXAMPLE
    ./folderstorm/scripts/package-release.ps1 -Exe target/release/aurora-viewer.exe -Mcp target/release/folderstorm-mcp.exe -Out folderstorm2-windows-x64
#>
param(
    [Parameter(Mandatory)][string]$Exe,
    [Parameter(Mandatory)][string]$Mcp,
    [Parameter(Mandatory)][string]$Out,
    [string]$Source = (Split-Path (Split-Path $PSScriptRoot -Parent) -Parent)
)
$ErrorActionPreference = 'Stop'

New-Item -ItemType Directory -Force -Path (Join-Path $Out 'assets/emoji') | Out-Null
Copy-Item -LiteralPath $Exe, $Mcp -Destination $Out
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
if (-not [IO.Path]::GetExtension($Exe) -and -not $IsMacOS) {
    # Linux: desktop entry and icon, as upstream's package.ps1
    Copy-Item -LiteralPath (Join-Path $Source 'packaging/linux/aurora-viewer.desktop') -Destination $Out
    Copy-Item -LiteralPath (Join-Path $Source 'assets/branding/aurora-logo.svg') -Destination (Join-Path $Out 'aurora-viewer.svg')
}
# "<product> <version>" -> the version (the last word)
$line = (& (Resolve-Path -LiteralPath $Exe).Path --version | Out-String).Trim()
$version = ($line -split '\s+')[-1]
if ($LASTEXITCODE -ne 0 -or $version -notmatch '^\d+\.\d+\.\d+([+-][0-9A-Za-z.+-]+)?$') { throw "Invalid package version: '$line'" }
$json = @{ schema = 1; version = $version } | ConvertTo-Json
[IO.File]::WriteAllText((Join-Path (Resolve-Path -LiteralPath $Out).Path 'aurora-install.json'), $json, [Text.UTF8Encoding]::new($false))
