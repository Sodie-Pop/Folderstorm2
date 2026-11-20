<#
.SYNOPSIS
    Compiles the Windows installer (folderstorm/installer/folderstorm2.iss)
    from a release package folder, and writes its .sha256. Used by
    build-release.ps1 and .github/workflows/folderstorm-release.yml.

.DESCRIPTION
    -Package is the folder package-release.ps1 -Tag made
    (<prefix>-<tag>-windows-x64): the installer lays down exactly those
    files. The names (product, executables, links, settings folders) come
    from crates/folderstorm-brand/src/lib.rs and reach Inno Setup as /D
    defines. Writes to -Destination (default: the folder that holds the
    package):
      <prefix>-<tag>-windows-x64-setup.exe
      <prefix>-<tag>-windows-x64-setup.exe.sha256   "<sha256>  <setup name>"
    and outputs their two paths. Unsigned for now (see the .iss).

    -Locate only finds Inno Setup's compiler and outputs its path: the -Iscc
    argument, $env:ISCC, ISCC.exe on PATH, then the default install folders
    (Program Files, and %LOCALAPPDATA%\Programs for winget's per-user
    install).

.EXAMPLE
    ./folderstorm/scripts/build-installer.ps1 -Package target/release-assets/v0.1.0/Folderstorm2-v0.1.0-windows-x64 -Tag v0.1.0
.EXAMPLE
    ./folderstorm/scripts/build-installer.ps1 -Locate
#>
#Requires -Version 7
[CmdletBinding(DefaultParameterSetName = 'Build')]
param(
    [Parameter(Mandatory, ParameterSetName = 'Build')][string]$Package,
    [Parameter(Mandatory, ParameterSetName = 'Build')][string]$Tag,
    [Parameter(ParameterSetName = 'Build')][string]$Destination,
    [Parameter(Mandatory, ParameterSetName = 'Locate')][switch]$Locate,
    # Inno Setup's command-line compiler (default: found, see -Locate)
    [string]$Iscc,
    [string]$Source = (Split-Path (Split-Path $PSScriptRoot -Parent) -Parent)
)
$ErrorActionPreference = 'Stop'

$installHint = 'Install Inno Setup 6: winget install --id JRSoftware.InnoSetup -e   (or: choco install innosetup)'

function Find-Iscc([string]$Given) {
    if ($Given) {
        if (Test-Path -LiteralPath $Given -PathType Leaf) { return (Resolve-Path -LiteralPath $Given).Path }
        throw "ISCC not found at $Given. $installHint"
    }
    if ($env:ISCC -and (Test-Path -LiteralPath $env:ISCC -PathType Leaf)) { return (Resolve-Path -LiteralPath $env:ISCC).Path }
    $onPath = Get-Command -Name 'ISCC.exe', 'iscc' -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($onPath) { return $onPath.Source }
    $roots = @(${env:ProgramFiles(x86)}, $env:ProgramFiles, $(if ($env:LOCALAPPDATA) { Join-Path $env:LOCALAPPDATA 'Programs' })) | Where-Object { $_ }
    foreach ($root in $roots) {
        # "Inno Setup 6", newest first should a later major version sit beside it
        $found = Get-ChildItem -LiteralPath $root -Directory -Filter 'Inno Setup*' -ErrorAction SilentlyContinue |
            Sort-Object Name -Descending |
            ForEach-Object { Join-Path $_.FullName 'ISCC.exe' } |
            Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } |
            Select-Object -First 1
        if ($found) { return $found }
    }
    throw "Inno Setup's compiler (ISCC.exe) was not found. $installHint"
}

$compiler = Find-Iscc $Iscc
if ($Locate) {
    Write-Output $compiler
    return
}

# The tags viewers take as releases (folderstorm/RELEASING.md).
if ($Tag -notmatch '^v((0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*))(-beta\.(0|[1-9]\d*))?$') { throw "Not a release version: '$Tag' (v0.1.0 or v0.2.0-beta.1)" }
$version = $Tag.Substring(1)
$numeric = "$($Matches[1]).0"

# The names come from folderstorm-brand, not from this file.
$brand = Get-Content -Raw -LiteralPath (Join-Path $Source 'crates/folderstorm-brand/src/lib.rs')
function Get-BrandConstant([string]$Name) {
    if ($brand -notmatch "(?m)^pub const ${Name}: &str = `"([^`"]+)`";") { throw "folderstorm-brand has no $Name" }
    $Matches[1]
}
if ($brand -notmatch 'macro_rules! product \{\s*\(\) => \{\s*"([^"]+)"') { throw 'folderstorm-brand has no product!()' }
$product = $Matches[1]
$exeName = Get-BrandConstant 'EXE'
$prefix = Get-BrandConstant 'RELEASE_PREFIX'

if (-not (Test-Path -LiteralPath $Package -PathType Container)) { throw "No package folder at $Package (package-release.ps1 -Tag makes it)" }
$Package = (Resolve-Path -LiteralPath $Package).Path
foreach ($file in $exeName, (Get-BrandConstant 'MCP_EXE'), 'aurora-install.json', 'LICENSE', 'assets') {
    if (-not (Test-Path -LiteralPath (Join-Path $Package $file))) { throw "The package folder has no $file`: make it with package-release.ps1 -Tag" }
}
# the package must be this release's: the updater reads the same file
$manifest = Get-Content -Raw -LiteralPath (Join-Path $Package 'aurora-install.json') | ConvertFrom-Json
if ($manifest.version -cne $version) { throw "The package is version '$($manifest.version)', not $version" }

if (-not $Destination) { $Destination = Split-Path -Parent $Package }
New-Item -ItemType Directory -Force -Path $Destination | Out-Null
$Destination = (Resolve-Path -LiteralPath $Destination).Path
$name = "$prefix-$Tag-windows-x64-setup.exe"
$setup = Join-Path $Destination $name
if (Test-Path -LiteralPath $setup) { throw "$setup already exists: remove it first" }

$defines = [ordered]@{
    AppVersion        = $version
    AppVersionNumeric = $numeric
    SourceDir         = $Package
    OutputDir         = $Destination
    AppName           = $product
    AppExe            = $exeName
    McpExe            = Get-BrandConstant 'MCP_EXE'
    ReleasePrefix     = $prefix
    Publisher         = Get-BrandConstant 'SITE_NAME'
    WebsiteUrl        = Get-BrandConstant 'WEBSITE'
    HelpUrl           = Get-BrandConstant 'HELP'
    BugsUrl           = Get-BrandConstant 'BUG_REPORTS'
    PrivacyUrl        = Get-BrandConstant 'PRIVACY'
    ReleasesUrl       = Get-BrandConstant 'RELEASES_PAGE'
    AumId             = Get-BrandConstant 'WINDOWS_AUMID'
    DirsOrganization  = Get-BrandConstant 'DIRS_ORGANIZATION'
    DirsApplication   = Get-BrandConstant 'DIRS_APPLICATION'
}
$arguments = @('/Qp') + @($defines.GetEnumerator() | ForEach-Object { "/D$($_.Key)=$($_.Value)" }) + @(Join-Path $Source 'folderstorm/installer/folderstorm2.iss')
& $compiler @arguments
if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed (exit code $LASTEXITCODE)" }
if (-not (Test-Path -LiteralPath $setup -PathType Leaf)) { throw "Inno Setup wrote no $name" }

# "<sha256>  <name>" and a line feed, UTF-8 without BOM, as the zip's
$hash = (Get-FileHash -LiteralPath $setup -Algorithm SHA256).Hash.ToLowerInvariant()
[IO.File]::WriteAllText("$setup.sha256", "$hash  $name`n", [Text.UTF8Encoding]::new($false))

$setup, "$setup.sha256" | ForEach-Object { Write-Output $_ }
