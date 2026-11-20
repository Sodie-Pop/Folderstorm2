<#
.SYNOPSIS
    Puts the built viewer, the MCP helper and the runtime files in a folder
    ready to zip; with -Tag, makes the exact assets of a GitHub release.
    Folderstorm2's counterpart of upstream's scripts/package.ps1, which
    cannot be used as is: it expects `--version` to print "Aurora Viewer
    <version>", and Folderstorm2 prints its own name. Same files, same
    aurora-install.json. Used by folderstorm/RELEASING.md and
    .github/workflows/folderstorm-release.yml.

.DESCRIPTION
    On Windows the viewer is copied under its shipped name (EXE of
    crates/folderstorm-brand/src/lib.rs, "Folderstorm2.exe"): the updater
    looks for that name, in the installation and in the package. Without
    -Out, the folder is named as the updater expects the package of this
    version: "<RELEASE_PREFIX>-v<version>-windows-x64" ("-linux-x64" for an
    executable without extension).

    With -Tag (Windows only), the viewer must report exactly that version
    (built with AURORA_VERSION_TAG on the tagged commit), and the script
    writes to -Destination (default target/release-assets/<tag>, ignored by git):
      <prefix>-<tag>-windows-x64/                     the package folder
      <prefix>-<tag>-windows-x64.zip                  the archive the updater installs
      <prefix>-<tag>-windows-x64.zip.sha256           "<sha256>  <zip name>"
      <prefix>-<tag>-windows-x64-symbols.zip          Breakpad symbols (dump_syms)
      <prefix>-<tag>-windows-x64-symbols.zip.sha256
    and outputs the paths of the four release assets. No update policy file:
    every update is optional (release plan, section 8).

    Without -Tag, the script outputs the folder's name.

.EXAMPLE
    ./folderstorm/scripts/package-release.ps1 -Exe target/release/aurora-viewer.exe -Mcp target/release/folderstorm-mcp.exe -Tag v0.1.0
.EXAMPLE
    ./folderstorm/scripts/package-release.ps1 -Exe target/release/aurora-viewer.exe -Mcp target/release/folderstorm-mcp.exe -Out folderstorm2-test-windows-x64
#>
param(
    [Parameter(Mandatory)][string]$Exe,
    [Parameter(Mandatory)][string]$Mcp,
    # default: the package folder of this version, named as the updater expects
    [string]$Out,
    # a release: v0.1.0 or v0.2.0-beta.1
    [string]$Tag,
    # where a release's files go (default target/release-assets/<tag>, ignored by git)
    [string]$Destination,
    # the Breakpad symbol tool: cargo install dump_syms --locked --version 2.3.9
    [string]$DumpSyms = 'dump_syms',
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
if ($brand -notmatch 'macro_rules! product \{\s*\(\) => \{\s*"([^"]+)"') { throw 'folderstorm-brand has no product!()' }
$product = $Matches[1]

$windows = [bool][IO.Path]::GetExtension($Exe)
if ($windows -and (Split-Path -Leaf $Mcp) -ne $mcpName) { throw "The MCP helper must be named $mcpName, not $(Split-Path -Leaf $Mcp)" }

# "<product> <version>" -> the version (the last word)
$line = (& (Resolve-Path -LiteralPath $Exe).Path --version | Out-String).Trim()
$version = ($line -split '\s+')[-1]
if ($LASTEXITCODE -ne 0 -or $version -notmatch '^\d+\.\d+\.\d+([+-][0-9A-Za-z.+-]+)?$') { throw "Invalid package version: '$line'" }

if ($Tag) {
    # The tags viewers take as releases: stable, or a Beta channel pre-release
    # (aurora-updater feed.rs); our own SemVer line (folderstorm/RELEASING.md).
    if ($Tag -notmatch '^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-beta\.(0|[1-9]\d*))?$') { throw "Not a release version: '$Tag' (v0.1.0 or v0.2.0-beta.1)" }
    if (-not $windows) { throw 'Release assets are made for Windows only' }
    if ($Out) { throw '-Out and -Tag do not go together: a release folder is named after its tag' }
    $expected = "$product $($Tag.Substring(1))"
    if ($line -cne $expected) {
        throw "Built version mismatch: expected '$expected', got '$line'. Tag the commit first, then build with AURORA_VERSION_TAG=$Tag."
    }
    if (-not $Destination) { $Destination = Join-Path 'target/release-assets' $Tag }
    if ((Test-Path -LiteralPath $Destination) -and (Get-ChildItem -LiteralPath $Destination -Force | Select-Object -First 1)) {
        throw "$Destination is not empty: remove it first"
    }
    New-Item -ItemType Directory -Force -Path $Destination | Out-Null
    $Destination = (Resolve-Path -LiteralPath $Destination).Path
    $Out = Join-Path $Destination "$prefix-$Tag-windows-x64"
}
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
if (-not $Tag) {
    Write-Output $Out
    return
}

# --- Release assets ---------------------------------------------------------
Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem

# A ZIP of everything under $Folder, each entry under "<Root>/" (when given)
# and named with `/`: the updater refuses `\` (Compress-Archive of Windows
# PowerShell 5.1 writes it).
function New-Zip([string]$Folder, [string]$Zip, [string]$Root) {
    $Folder = (Resolve-Path -LiteralPath $Folder).Path
    $archive = [IO.Compression.ZipFile]::Open($Zip, [IO.Compression.ZipArchiveMode]::Create)
    try {
        foreach ($file in Get-ChildItem -LiteralPath $Folder -Recurse -File) {
            $entry = $file.FullName.Substring($Folder.Length + 1).Replace('\', '/')
            if ($Root) { $entry = "$Root/$entry" }
            [void][IO.Compression.ZipFileExtensions]::CreateEntryFromFile($archive, $file.FullName, $entry, [IO.Compression.CompressionLevel]::Optimal)
        }
    } finally {
        $archive.Dispose()
    }
}

# "<sha256>  <name>" and a line feed, UTF-8 without BOM: what aurora-updater's
# package.rs parse_checksum reads.
function Write-Checksum([string]$File) {
    $hash = (Get-FileHash -LiteralPath $File -Algorithm SHA256).Hash.ToLowerInvariant()
    [IO.File]::WriteAllText("$File.sha256", "$hash  $(Split-Path -Leaf $File)`n", [Text.UTF8Encoding]::new($false))
}

$name = Split-Path -Leaf $Out
$zip = Join-Path $Destination "$name.zip"
New-Zip -Folder $Out -Zip $zip -Root $name
Write-Checksum $zip

# Breakpad symbols of the viewer for the crash reports, as upstream's
# release.yml: dump_syms reads the PDB the exe records (cargo's
# target/release/aurora_viewer.pdb) and stores
# aurora_viewer.pdb/<debug id>/aurora_viewer.sym. The PDB is not published.
$symbolsDir = Join-Path $Destination 'symbols'
& $DumpSyms (Resolve-Path -LiteralPath $Exe).Path --store $symbolsDir
if ($LASTEXITCODE -ne 0) { throw "dump_syms failed (install it: cargo install dump_syms --locked --version 2.3.9)" }
if (-not (Get-ChildItem -LiteralPath $symbolsDir -Recurse -File -Filter '*.sym')) { throw 'dump_syms wrote no symbols' }
$symbols = Join-Path $Destination "$name-symbols.zip"
New-Zip -Folder $symbolsDir -Zip $symbols
Write-Checksum $symbols
Remove-Item -LiteralPath $symbolsDir -Recurse -Force

$zip, "$zip.sha256", $symbols, "$symbols.sha256" | ForEach-Object { Write-Output $_ }
