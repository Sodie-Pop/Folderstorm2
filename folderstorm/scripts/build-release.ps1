<#
.SYNOPSIS
    Builds a whole Folderstorm2 release in one go, on the human's Windows PC:
    checks, local tag, release build, zip, symbols, installer, checksums and,
    with -Draft, a DRAFT GitHub release. folderstorm/RELEASING.md explains it.

.DESCRIPTION
    In order, stopping with a clear message at the first problem:
      1. checks: on main, clean tree, up to date with origin/main; the tag
         exists neither here nor on origin, and is higher than the last
         release; the version is a release version; cargo, dump_syms,
         Inno Setup (and gh, signed in, for -Draft) are installed;
      2. the local tag (never pushed: GitHub makes the real one when the
         human publishes the draft);
      3. cargo build --release --locked -p aurora-viewer -p folderstorm-mcp
         --features aurora-viewer/xr-static-loader, with AURORA_VERSION_TAG;
      4. package-release.ps1 -Tag: the zip, the symbols and their .sha256;
      5. build-installer.ps1: the setup .exe and its .sha256;
      6. a summary: each asset with its size and SHA-256, and the gh command;
      7. with -Draft: gh release create <tag> --draft (--prerelease for a
         beta) with the six assets. Never published, never --latest: the
         human reviews the draft and publishes it on GitHub.

    The files go to <target>/release-assets/<tag>/ (<target> is
    $env:CARGO_TARGET_DIR, or target/). After a failure, fix the cause and
    run again with -Clean: it removes the local tag and that folder first.

    -DryRun runs every check, then prints what it would do (tag, commands,
    assets, gh command) without changing anything. gh is signed in by the
    human: this script never asks for, reads or stores a token.

.EXAMPLE
    ./folderstorm/scripts/build-release.ps1 -Version 0.1.0
.EXAMPLE
    ./folderstorm/scripts/build-release.ps1 -Version 0.2.0 -Beta 1 -Draft
.EXAMPLE
    ./folderstorm/scripts/build-release.ps1 -Version v0.2.0-beta.1 -Draft -Clean
#>
#Requires -Version 7
param(
    # 0.1.0, v0.1.0, 0.2.0-beta.1 or v0.2.0-beta.1
    [Parameter(Mandatory, Position = 0)][string]$Version,
    # with a plain -Version: the Nth beta of it (0.2.0 -Beta 1 = v0.2.0-beta.1)
    [int]$Beta = -1,
    # create the draft GitHub release (needs gh, signed in by the human)
    [switch]$Draft,
    # remove the local tag and the release folder of an earlier attempt first
    [switch]$Clean,
    # check everything, change nothing
    [switch]$DryRun,
    # the human's own release notes, put after folderstorm/release-notes-header.md
    [string]$NotesFile,
    # the Breakpad symbol tool (cargo install dump_syms --locked --version 2.3.9)
    [string]$DumpSyms = 'dump_syms',
    # Inno Setup's compiler (default: found by build-installer.ps1 -Locate)
    [string]$Iscc
)
$ErrorActionPreference = 'Stop'

function Stop-Release([string]$Message) {
    throw "build-release: $Message"
}
function Write-Step([string]$Text) {
    Write-Host ''
    Write-Host "== $Text" -ForegroundColor Cyan
}
# A native command, stopping on failure.
function Invoke-Checked([string]$What, [scriptblock]$Command) {
    & $Command
    if ($LASTEXITCODE -ne 0) { Stop-Release "$What failed (exit code $LASTEXITCODE)" }
}
# A command line as it can be pasted in PowerShell.
function Format-Command([string[]]$Words) {
    ($Words | ForEach-Object {
            if ($_ -match '^[A-Za-z0-9_./:=\\+-]+$') { $_ } else { "'" + ($_ -replace "'", "''") + "'" }
        }) -join ' '
}
# major.minor.patch, then a stable version above its betas, as the updater orders them.
function Get-VersionKey([string]$Tag) {
    if ($Tag -notmatch '^v(\d+)\.(\d+)\.(\d+)(-beta\.(\d+))?$') { return $null }
    $pre = if ($Matches[5]) { [long]$Matches[5] } else { [long]::MaxValue }
    , @([long]$Matches[1], [long]$Matches[2], [long]$Matches[3], $pre)
}
function Compare-Version([string]$A, [string]$B) {
    $ka = Get-VersionKey $A
    $kb = Get-VersionKey $B
    for ($i = 0; $i -lt 4; $i++) {
        if ($ka[$i] -ne $kb[$i]) { return [Math]::Sign($ka[$i] - $kb[$i]) }
    }
    0
}

$started = $false
$built = $false
$repo = (& git rev-parse --show-toplevel 2>$null)
if ($LASTEXITCODE -ne 0 -or -not $repo) { Stop-Release 'run it from inside the Folderstorm2 repository' }
$repo = $repo.Trim()
Push-Location -LiteralPath $repo
try {
    # --- The tag ---------------------------------------------------------------
    $tag = $Version.Trim()
    if (-not $tag.StartsWith('v')) { $tag = "v$tag" }
    if ($Beta -ge 0) {
        if ($tag -match '-') { Stop-Release "-Beta goes with a plain version (0.2.0 -Beta 1), not with $Version" }
        $tag = "$tag-beta.$Beta"
    }
    # Our own SemVer line: stable, or a Beta channel pre-release (RELEASING.md section 1).
    if ($tag -notmatch '^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-beta\.(0|[1-9]\d*))?$') {
        Stop-Release "not a release version: '$tag'. Use 0.1.0, or a beta: 0.2.0 -Beta 1 (v0.2.0-beta.1). No other pre-release word, no build metadata."
    }
    $isBeta = $tag.Contains('-')
    Write-Host "Release $tag ($(if ($isBeta) { 'beta: GitHub pre-release' } else { 'stable' }))$(if ($DryRun) { ', dry run: nothing is changed' })"

    $windows = $env:OS -eq 'Windows_NT'
    if (-not $windows -and -not $DryRun) { Stop-Release 'a release is built on Windows (run with -DryRun elsewhere to try the checks)' }

    # --- Names and places ----------------------------------------------------------
    $brand = Get-Content -Raw -LiteralPath (Join-Path $repo 'crates/folderstorm-brand/src/lib.rs')
    function Get-BrandConstant([string]$Name) {
        if ($brand -notmatch "(?m)^pub const ${Name}: &str = `"([^`"]+)`";") { Stop-Release "folderstorm-brand has no $Name" }
        $Matches[1]
    }
    if ($brand -notmatch 'macro_rules! product \{\s*\(\) => \{\s*"([^"]+)"') { Stop-Release 'folderstorm-brand has no product!()' }
    $product = $Matches[1]
    $prefix = Get-BrandConstant 'RELEASE_PREFIX'
    $github = Get-BrandConstant 'REPOSITORY'
    $target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $repo 'target' }
    $dist = Join-Path (Join-Path $target 'release-assets') $tag
    $name = "$prefix-$tag-windows-x64"
    $assets = @(
        (Join-Path $dist "$name.zip"),
        (Join-Path $dist "$name.zip.sha256"),
        (Join-Path $dist "$name-symbols.zip"),
        (Join-Path $dist "$name-symbols.zip.sha256"),
        (Join-Path $dist "$name-setup.exe"),
        (Join-Path $dist "$name-setup.exe.sha256")
    )
    $notes = Join-Path $dist 'release-notes.md'
    if ($NotesFile -and -not (Test-Path -LiteralPath $NotesFile -PathType Leaf)) { Stop-Release "no notes file at $NotesFile" }

    # --- 1. Checks -----------------------------------------------------------------
    Write-Step 'Checks'
    $localTag = & git tag --list $tag
    $assetsLeft = (Test-Path -LiteralPath $dist) -and [bool](Get-ChildItem -LiteralPath $dist -Force | Select-Object -First 1)

    Invoke-Checked 'git fetch origin main' { git fetch --quiet origin main }
    $branch = (& git symbolic-ref --quiet --short HEAD)
    if ($branch -ne 'main') { Stop-Release "on branch '$branch': switch to main (git switch main)" }
    $dirty = & git status --porcelain
    if ($dirty) { Stop-Release "the working tree is not clean (git status):`n$($dirty -join "`n")" }
    $head = (& git rev-parse HEAD).Trim()
    $originMain = (& git rev-parse origin/main).Trim()
    if ($head -ne $originMain) { Stop-Release "main is not origin/main ($($head.Substring(0, 8)) vs $($originMain.Substring(0, 8))): git pull --ff-only origin main, or push your commits first" }
    Write-Host "main is up to date with origin/main at $($head.Substring(0, 8))"

    # the tags on origin: this one must not exist, and must be higher than every release
    $remoteTags = & git ls-remote --tags --refs origin 'v*'
    if ($LASTEXITCODE -ne 0) { Stop-Release 'git ls-remote origin failed' }
    $released = @($remoteTags | ForEach-Object { ($_ -split '\s+')[1] -replace '^refs/tags/', '' } | Where-Object { Get-VersionKey $_ })
    if ($released -contains $tag) { Stop-Release "$tag is already released on origin: choose a higher version" }
    $latest = $released | Sort-Object -Property { (Get-VersionKey $_)[0] }, { (Get-VersionKey $_)[1] }, { (Get-VersionKey $_)[2] }, { (Get-VersionKey $_)[3] } | Select-Object -Last 1
    if ($latest -and (Compare-Version $tag $latest) -le 0) { Stop-Release "$tag is not higher than the last release, ${latest}: viewers would not take it" }
    Write-Host "$tag is new$(if ($latest) { " (last release: $latest)" } else { ' (no release yet)' })"

    if ($localTag -and -not $Clean) { Stop-Release "the tag $tag already exists here (an earlier attempt?): run again with -Clean" }
    if ($assetsLeft -and -not $Clean) { Stop-Release "$dist is not empty (an earlier attempt?): run again with -Clean" }

    $cargo = Get-Command cargo -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $cargo) { Stop-Release 'cargo not found: install Rust from https://rustup.rs' }
    $dumpSymsCommand = Get-Command $DumpSyms -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $dumpSymsCommand) { Stop-Release "dump_syms not found: cargo install dump_syms --locked --version 2.3.9" }
    try {
        $isccPath = & (Join-Path $PSScriptRoot 'build-installer.ps1') -Locate -Iscc $Iscc
    } catch {
        Stop-Release $_.Exception.Message
    }
    Write-Host "cargo: $($cargo.Source)"
    Write-Host "dump_syms: $($dumpSymsCommand.Source)"
    Write-Host "Inno Setup: $isccPath"
    if ($Draft) {
        $gh = Get-Command gh -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
        if (-not $gh) { Stop-Release 'gh not found (needed for -Draft): winget install --id GitHub.cli -e, then ask the human to sign it in' }
        # exit code only: the output is not shown
        & gh auth status --hostname github.com *> $null
        if ($LASTEXITCODE -ne 0) { Stop-Release 'gh is not signed in: ask the human to run gh auth login (never type a token yourself)' }
        & gh release view $tag --repo $github *> $null
        if ($LASTEXITCODE -eq 0) { Stop-Release "a release or draft named $tag already exists on GitHub: the human deletes the draft there first" }
        Write-Host "gh: $($gh.Source), signed in"
    }

    if ($Clean) {
        Write-Step 'Clean'
        if ($localTag) {
            if ($DryRun) { Write-Host "would remove the local tag $tag" } else { Invoke-Checked 'git tag -d' { git tag -d $tag } }
        }
        if (Test-Path -LiteralPath $dist) {
            if ($DryRun) { Write-Host "would remove $dist" } else { Remove-Item -LiteralPath $dist -Recurse -Force; Write-Host "removed $dist" }
        }
    }

    $features = 'aurora-viewer/xr-static-loader'
    $build = @('cargo', 'build', '--release', '--locked', '-p', 'aurora-viewer', '-p', 'folderstorm-mcp', '--features', $features)
    $exe = Join-Path $target 'release/aurora-viewer.exe'
    $mcp = Join-Path $target 'release/folderstorm-mcp.exe'
    $ghArgs = @('release', 'create', $tag) + $assets + @('--repo', $github, '--draft', '--target', $head, '--title', "$product $tag", '--notes-file', $notes, '--generate-notes')
    if ($isBeta) { $ghArgs += '--prerelease' }

    if ($DryRun) {
        Write-Step 'Dry run: what a real run does'
        Write-Host "2. git tag $tag   (at $head, local only)"
        Write-Host "3. `$env:AURORA_VERSION_TAG = '$tag'; $(Format-Command $build)"
        Write-Host "4. $(Format-Command @('package-release.ps1', '-Exe', $exe, '-Mcp', $mcp, '-Tag', $tag, '-Destination', $dist, '-DumpSyms', $dumpSymsCommand.Source))"
        Write-Host "5. $(Format-Command @('build-installer.ps1', '-Package', (Join-Path $dist $name), '-Tag', $tag, '-Destination', $dist, '-Iscc', $isccPath))"
        Write-Host '6. the release assets:'
        $assets | ForEach-Object { Write-Host "     $_" }
        Write-Host "   notes: $notes (folderstorm/release-notes-header.md$(if ($NotesFile) { " + $NotesFile" }), then GitHub's generated notes)"
        Write-Host "7. $(if ($Draft) { '' } else { '(with -Draft) ' })$(Format-Command (@('gh') + $ghArgs))"
        Write-Host ''
        Write-Host 'Dry run done: every check passed, nothing was changed.' -ForegroundColor Green
        return
    }

    $started = $true
    # --- 2. Tag --------------------------------------------------------------------
    Write-Step "Tag $tag (local only, never pushed)"
    Invoke-Checked 'git tag' { git tag $tag }

    # --- 3. Build ------------------------------------------------------------------
    # The release profile: this build ships. aurora-version takes the version
    # from AURORA_VERSION_TAG only on the tagged commit itself.
    Write-Step 'Build (release profile, this takes a while)'
    $previousTag = $env:AURORA_VERSION_TAG
    $env:AURORA_VERSION_TAG = $tag
    try {
        Invoke-Checked 'cargo build' { & $build[0] $build[1..($build.Count - 1)] }
    } finally {
        if ($null -eq $previousTag) { Remove-Item Env:AURORA_VERSION_TAG -ErrorAction SilentlyContinue } else { $env:AURORA_VERSION_TAG = $previousTag }
    }

    # --- 4. Package: zip, symbols, checksums -------------------------------------
    Write-Step 'Package (zip, symbols, checksums)'
    $packaged = & (Join-Path $PSScriptRoot 'package-release.ps1') -Exe $exe -Mcp $mcp -Tag $tag -Destination $dist -DumpSyms $dumpSymsCommand.Source

    # --- 5. Installer ------------------------------------------------------------
    Write-Step 'Installer (Inno Setup)'
    $installer = & (Join-Path $PSScriptRoot 'build-installer.ps1') -Package (Join-Path $dist $name) -Tag $tag -Destination $dist -Iscc $isccPath

    # every asset is there, and every checksum matches its file
    $made = @($packaged) + @($installer) | ForEach-Object { (Resolve-Path -LiteralPath $_).Path }
    foreach ($asset in $assets) {
        if ($made -notcontains (Resolve-Path -LiteralPath $asset -ErrorAction SilentlyContinue).Path) { Stop-Release "missing release asset: $asset" }
    }
    foreach ($file in $assets | Where-Object { -not $_.EndsWith('.sha256') }) {
        $line = (Get-Content -Raw -LiteralPath "$file.sha256").Trim()
        $hash = (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($line -cne "$hash  $(Split-Path -Leaf $file)") { Stop-Release "checksum mismatch: $file" }
    }
    $header = Get-Content -Raw -LiteralPath (Join-Path $repo 'folderstorm/release-notes-header.md')
    $text = $header.TrimEnd() + "`n"
    if ($NotesFile) { $text += "`n" + (Get-Content -Raw -LiteralPath $NotesFile).TrimEnd() + "`n" }
    [IO.File]::WriteAllText($notes, $text, [Text.UTF8Encoding]::new($false))

    # --- 6. Summary --------------------------------------------------------------------
    Write-Step "Release $tag is built"
    foreach ($file in $assets | Where-Object { -not $_.EndsWith('.sha256') }) {
        $item = Get-Item -LiteralPath $file
        $hash = (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant()
        Write-Host ("{0}`n    {1:N1} MB   SHA-256 {2}" -f $item.FullName, ($item.Length / 1MB), $hash)
        Write-Host "  $($item.FullName).sha256"
    }
    Write-Host ''
    Write-Host "The package folder $(Join-Path $dist $name) is not uploaded; try it: & '$(Join-Path (Join-Path $dist $name) (Get-BrandConstant 'EXE'))' --version"
    Write-Host ''
    Write-Host 'The draft release (gh signed in by the human):'
    Write-Host "  $(Format-Command (@('gh') + $ghArgs))"
    $built = $true

    # --- 7. Draft ----------------------------------------------------------------------
    if ($Draft) {
        Write-Step 'Draft GitHub release'
        Invoke-Checked 'gh release create' { & gh @ghArgs }
        Write-Host ''
        Write-Host "Draft $tag created, not published. The human reviews it (title, notes, six assets$(if ($isBeta) { ', pre-release box ticked' })) and clicks Publish release:" -ForegroundColor Green
        Write-Host "  gh release view $tag --repo $github --web"
    } else {
        Write-Host ''
        Write-Host 'Run again with -Draft to create the draft, or paste the command above.' -ForegroundColor Green
    }
} catch {
    Write-Host ''
    Write-Host $_.Exception.Message -ForegroundColor Red
    if ($built) {
        Write-Host 'The release files are built: fix the cause, then paste the gh command above (no need to build again).' -ForegroundColor Yellow
    } elseif ($started) {
        Write-Host "Fix the cause, then run again with -Clean (it removes the local tag $tag and $dist first)." -ForegroundColor Yellow
    }
    exit 1
} finally {
    Pop-Location
}
