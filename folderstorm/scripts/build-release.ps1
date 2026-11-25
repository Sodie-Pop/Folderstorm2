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
         then the zip's .sha256 signed with the maintainer's update key
         (scripts/sign-release.ps1: <zip>.sha256.sig, which the updater
         checks against folderstorm-brand's UPDATE_KEYS);
      5. build-installer.ps1: the setup .exe and its .sha256;
      6. a summary: each asset with its size and SHA-256, and the gh command;
      7. with -Draft: gh release create <tag> --draft (--prerelease for a
         beta) with the seven assets. Never published, never --latest: the
         human reviews the draft and publishes it on GitHub.

    The files go to <target>/release-assets/<tag>/ (<target> is
    $env:CARGO_TARGET_DIR, or target/). After a failure, fix the cause and
    run again with -Clean: it removes the local tag and that folder first.

    -DryRun runs every check, then prints what it would do (tag, commands,
    assets, gh command) without changing anything. gh is signed in by the
    human: this script never asks for, reads or stores a token.

    Test, then release the same files (two runs, no second build):
      -Candidate, on any branch with a clean tree (a PR branch): the same
         build, package, symbols, installer and checksums, then
         release-candidate.json beside the assets (version, tag, commit,
         tree hash, each asset's SHA-256, build date). The local tag is only
         there for the build, and removed right after it. No draft.
      -FromExisting, on main after the merge: the usual checks, then it
         refuses unless HEAD's tree is the candidate's tree (the merged code
         is byte for byte what was built), and unless every asset matches
         the candidate's SHA-256 and its .sha256 file. Then the local tag
         and, with -Draft, the draft, with those files. Nothing is built.
    Both use <main checkout>/target/release-assets/<tag>/ (or
    $env:CARGO_TARGET_DIR's): a task's worktree, and its target\, go when
    the task ends, and main's run must find the files.

.EXAMPLE
    ./folderstorm/scripts/build-release.ps1 -Version 0.1.0
.EXAMPLE
    ./folderstorm/scripts/build-release.ps1 -Version 0.2.0 -Beta 1 -Draft
.EXAMPLE
    ./folderstorm/scripts/build-release.ps1 -Version v0.2.0-beta.1 -Draft -Clean
.EXAMPLE
    ./folderstorm/scripts/build-release.ps1 -Version 0.2.0 -Candidate   # on the PR branch
.EXAMPLE
    ./folderstorm/scripts/build-release.ps1 -Version 0.2.0 -FromExisting -Draft   # on main, after the merge
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
    # build the release files to test, on any branch (a PR branch): no draft
    [switch]$Candidate,
    # on main after the merge: release the files -Candidate built, without building
    [switch]$FromExisting,
    # the human's own release notes, put after folderstorm/release-notes-header.md
    [string]$NotesFile,
    # the Breakpad symbol tool (cargo install dump_syms --locked --version 2.3.9)
    [string]$DumpSyms = 'dump_syms',
    # Inno Setup's compiler (default: found by build-installer.ps1 -Locate)
    [string]$Iscc,
    # the maintainer's Ed25519 update key (PEM); default: $env:FOLDERSTORM2_UPDATE_KEY,
    # else folderstorm2-update-key.pem in the user's folder (RELEASING.md, "Signing key")
    [string]$SigningKey
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
function Get-Sha256([string]$File) {
    (Get-FileHash -LiteralPath $File -Algorithm SHA256).Hash.ToLowerInvariant()
}
# Each file's .sha256 holds "<sha256>  <name>", the line the updater checks.
function Assert-Checksums([string[]]$Files) {
    foreach ($file in $Files | Where-Object { -not $_.EndsWith('.sha256') -and -not $_.EndsWith('.sig') }) {
        $line = (Get-Content -Raw -LiteralPath "$file.sha256").Trim()
        if ($line -cne "$(Get-Sha256 $file)  $(Split-Path -Leaf $file)") { Stop-Release "checksum mismatch: $file" }
    }
}

$started = $false
$built = $false
$tagged = $false
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
    if ($Candidate -and $FromExisting) { Stop-Release '-Candidate builds the files to test, -FromExisting releases them after the merge: one or the other' }
    if ($Candidate -and $Draft) { Stop-Release 'a candidate is never a draft: merge the PR, then run -FromExisting -Draft on main' }
    if ($FromExisting -and $Clean) { Stop-Release '-Clean would delete the tested files: -FromExisting reuses a local tag already at HEAD, and otherwise asks you to remove it' }
    $mode = if ($Candidate) { ', candidate to test: no draft' } elseif ($FromExisting) { ', from the tested candidate: no build' } else { '' }
    Write-Host "Release $tag ($(if ($isBeta) { 'beta: GitHub pre-release' } else { 'stable' }))$mode$(if ($DryRun) { ', dry run: nothing is changed' })"

    $windows = $env:OS -eq 'Windows_NT'
    if (-not $windows -and -not $DryRun -and -not $FromExisting) { Stop-Release 'a release is built on Windows (run with -DryRun elsewhere to try the checks)' }

    # --- Names and places ----------------------------------------------------------
    $brand = Get-Content -Raw -LiteralPath (Join-Path $repo 'crates/folderstorm-brand/src/lib.rs')
    function Get-BrandConstant([string]$Name) {
        if ($brand -notmatch "(?m)^pub const ${Name}: &str = `"([^`"]+)`";") { Stop-Release "folderstorm-brand has no $Name" }
        $Matches[1]
    }
    if ($brand -notmatch 'macro_rules! product \{\s*\(\) => \{\s*"([^"]+)"') { Stop-Release 'folderstorm-brand has no product!()' }
    $product = $Matches[1]
    $prefix = Get-BrandConstant 'RELEASE_PREFIX'
    # the public keys the updater trusts: a release signed with another key is never offered
    if ($brand -notmatch '(?s)pub const UPDATE_KEYS: &\[&str\] = &\[(.*?)\];') { Stop-Release 'folderstorm-brand has no UPDATE_KEYS' }
    $updateKeys = @([regex]::Matches($Matches[1], '"([0-9a-f]{64})"') | ForEach-Object { $_.Groups[1].Value })
    if ($updateKeys.Count -eq 0) { Stop-Release 'folderstorm-brand lists no UPDATE_KEYS' }
    $github = Get-BrandConstant 'REPOSITORY'
    $target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $repo 'target' }
    $assetsRoot = $target
    if (($Candidate -or $FromExisting) -and -not $env:CARGO_TARGET_DIR) {
        # A task's worktree and its target\ go when the task ends
        # (scripts/end-task.ps1, new-task.ps1): a candidate's files live in
        # the main checkout's target\, where main's -FromExisting finds them.
        $common = & git rev-parse --path-format=absolute --git-common-dir 2>$null
        if ($LASTEXITCODE -eq 0 -and $common -and (Split-Path -Leaf $common.Trim()) -eq '.git') {
            $assetsRoot = Join-Path (Split-Path -Parent $common.Trim()) 'target'
        }
    }
    $dist = Join-Path (Join-Path $assetsRoot 'release-assets') $tag
    $manifestPath = Join-Path $dist 'release-candidate.json'
    $name = "$prefix-$tag-windows-x64"
    $assets = @(
        (Join-Path $dist "$name.zip"),
        (Join-Path $dist "$name.zip.sha256"),
        (Join-Path $dist "$name.zip.sha256.sig"),
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
    if (-not $Candidate -and $branch -ne 'main') { Stop-Release "on branch '$branch': switch to main (git switch main)" }
    $dirty = & git status --porcelain
    if ($dirty) { Stop-Release "the working tree is not clean (git status):`n$($dirty -join "`n")" }
    $head = (& git rev-parse HEAD).Trim()
    $tree = (& git rev-parse 'HEAD^{tree}').Trim()
    $originMain = (& git rev-parse origin/main).Trim()
    if ($Candidate) {
        Write-Host "candidate from $(if ($branch) { "branch '$branch'" } else { 'a detached HEAD' }) at $($head.Substring(0, 8)), tree $($tree.Substring(0, 8))"
        # The merge of a branch that contains main has this very tree.
        & git merge-base --is-ancestor $originMain $head
        if ($LASTEXITCODE -ne 0) {
            Write-Warning "this branch does not contain origin/main ($($originMain.Substring(0, 8))): its merge will not have the tree built here, and -FromExisting will refuse. Update the branch from main first, unless this candidate is only for a try."
        }
    } else {
        if ($head -ne $originMain) { Stop-Release "main is not origin/main ($($head.Substring(0, 8)) vs $($originMain.Substring(0, 8))): git pull --ff-only origin main, or push your commits first" }
        Write-Host "main is up to date with origin/main at $($head.Substring(0, 8))"
    }

    # the tags on origin: this one must not exist, and must be higher than every release
    $remoteTags = & git ls-remote --tags --refs origin 'v*'
    if ($LASTEXITCODE -ne 0) { Stop-Release 'git ls-remote origin failed' }
    $released = @($remoteTags | ForEach-Object { ($_ -split '\s+')[1] -replace '^refs/tags/', '' } | Where-Object { Get-VersionKey $_ })
    if ($released -contains $tag) { Stop-Release "$tag is already released on origin: choose a higher version" }
    $latest = $released | Sort-Object -Property { (Get-VersionKey $_)[0] }, { (Get-VersionKey $_)[1] }, { (Get-VersionKey $_)[2] }, { (Get-VersionKey $_)[3] } | Select-Object -Last 1
    if ($latest -and (Compare-Version $tag $latest) -le 0) { Stop-Release "$tag is not higher than the last release, ${latest}: viewers would not take it" }
    Write-Host "$tag is new$(if ($latest) { " (last release: $latest)" } else { ' (no release yet)' })"

    if ($FromExisting) {
        if ($localTag) {
            $tagAt = (& git rev-parse "refs/tags/$tag^{commit}").Trim()
            if ($tagAt -ne $head) { Stop-Release "the tag $tag already exists here at $($tagAt.Substring(0, 8)), not at HEAD: remove it (git tag -d $tag), then run again" }
            Write-Host "the local tag $tag is already at HEAD (an earlier attempt): kept"
        }
    } else {
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

        # the update key: its file only, never its content in the output
        $home_ = if ($env:USERPROFILE) { $env:USERPROFILE } else { $HOME }
        $keyFile = if ($SigningKey) { $SigningKey } elseif ($env:FOLDERSTORM2_UPDATE_KEY) { $env:FOLDERSTORM2_UPDATE_KEY } else { Join-Path $home_ 'folderstorm2-update-key.pem' }
        if (-not (Test-Path -LiteralPath $keyFile -PathType Leaf)) {
            $message = "no update signing key at ${keyFile}: viewers would not offer an unsigned release. Give its path with -SigningKey (RELEASING.md, `"Signing key`")"
            if ($DryRun) { Write-Warning $message } else { Stop-Release $message }
        } else {
            Write-Host "update signing key: $keyFile"
        }
        $openssl = Get-Command openssl -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
        if (-not $openssl) {
            $git = Get-Command git -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
            $found = if ($git) { @('mingw64\bin\openssl.exe', 'usr\bin\openssl.exe') | ForEach-Object { Join-Path (Split-Path (Split-Path $git.Source)) $_ } | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1 }
            if (-not $found) { Stop-Release 'openssl not found (it comes with Git for Windows): needed to sign the release' }
        }
    }
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

    if ($FromExisting) {
        # --- The candidate: main's code is what was built, the files are what was tested
        Write-Step "The tested candidate ($dist)"
        if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
            Stop-Release "no candidate manifest at ${manifestPath}: build the candidate on the PR branch with -Candidate first (or release without -FromExisting, which builds again on main)"
        }
        try {
            $manifest = Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json
        } catch {
            Stop-Release "$manifestPath is not valid JSON: build the candidate again (-Candidate -Clean)"
        }
        if ($manifest.schema -ne 1 -or "$($manifest.tree)" -notmatch '^[0-9a-f]{40,64}$' -or "$($manifest.commit)" -notmatch '^[0-9a-f]{40,64}$' -or -not $manifest.assets) {
            Stop-Release "$manifestPath is not a candidate manifest this script reads: build the candidate again (-Candidate -Clean)"
        }
        if ($manifest.tag -cne $tag) { Stop-Release "the candidate in $dist is $($manifest.tag), not $tag" }
        $candidateCommit = "$($manifest.commit)"
        Write-Host "candidate $tag built $($manifest.built) from $(if ($manifest.branch) { "'$($manifest.branch)' at " })$($candidateCommit.Substring(0, 8))"

        if ($tree -cne $manifest.tree) {
            Write-Host ''
            Write-Host "main's code is not the code the candidate was built from (tree $($tree.Substring(0, 8)) here, $($manifest.tree.Substring(0, 8)) in the candidate)." -ForegroundColor Red
            & git cat-file -e "$candidateCommit^{commit}" 2>$null
            if ($LASTEXITCODE -eq 0) {
                Write-Host "What differs (git diff --stat $($candidateCommit.Substring(0, 8)) HEAD):"
                & git diff --stat $candidateCommit HEAD | Out-Host
            } else {
                Write-Host "The candidate's commit $($candidateCommit.Substring(0, 8)) is not in this clone: git fetch origin, then git diff --stat $($candidateCommit.Substring(0, 8)) HEAD shows what differs."
            }
            Stop-Release 'main is not byte for byte what was tested (another PR merged in between, the PR branch was behind main, or it changed after the candidate): nothing released. Build and test a new candidate (-Candidate -Clean on a branch up to date with main), or release without -FromExisting, which builds again on main.'
        }
        Write-Host "main's tree is the candidate's: the merged code is what was built"

        # the same seven names, each file with the recorded SHA-256 and a matching .sha256
        $names = @($assets | ForEach-Object { Split-Path -Leaf $_ })
        $listed = @($manifest.assets | ForEach-Object { "$($_.name)" })
        if ($listed.Count -ne $names.Count -or (Compare-Object -ReferenceObject $names -DifferenceObject $listed -CaseSensitive)) {
            Stop-Release "the candidate lists other assets than the seven of ${tag} (a candidate built before release signing? build it again): $($listed -join ', ')"
        }
        foreach ($file in $assets) {
            $leaf = Split-Path -Leaf $file
            if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { Stop-Release "missing release asset: $file" }
            $recorded = "$(($manifest.assets | Where-Object { $_.name -ceq $leaf } | Select-Object -First 1).sha256)"
            $hash = Get-Sha256 $file
            if ($hash -cne $recorded) { Stop-Release "$leaf is not the file that was tested (SHA-256 $hash, the candidate recorded $recorded): build and test a new candidate" }
        }
        Assert-Checksums $assets
        Write-Host 'the seven assets match the candidate and their .sha256 files'
    }

    $features = 'aurora-viewer/xr-static-loader'
    $build = @('cargo', 'build', '--release', '--locked', '-p', 'aurora-viewer', '-p', 'folderstorm-mcp', '--features', $features)
    $exe = Join-Path $target 'release/aurora-viewer.exe'
    $mcp = Join-Path $target 'release/folderstorm-mcp.exe'
    $ghArgs = @('release', 'create', $tag) + $assets + @('--repo', $github, '--draft', '--target', $head, '--title', "$product $tag", '--notes-file', $notes, '--generate-notes')
    if ($isBeta) { $ghArgs += '--prerelease' }

    if ($DryRun) {
        Write-Step 'Dry run: what a real run does'
        if ($FromExisting) {
            Write-Host "2. $(if ($localTag) { "keep the local tag $tag" } else { "git tag $tag" })   (at $head, local only); nothing is built"
            Write-Host '3. the release assets, as tested:'
            $assets | ForEach-Object { Write-Host "     $_" }
            Write-Host "   notes: $notes ($(if ($NotesFile) { "folderstorm/release-notes-header.md + $NotesFile" } else { "the candidate's, else folderstorm/release-notes-header.md" }), then GitHub's generated notes)"
            Write-Host "4. $(if ($Draft) { '' } else { '(with -Draft) ' })$(Format-Command (@('gh') + $ghArgs))"
            Write-Host ''
            Write-Host 'Dry run done: every check passed, nothing was changed.' -ForegroundColor Green
            return
        }
        Write-Host "2. git tag $tag   (at $head, local only$(if ($Candidate) { ', removed right after the build' }))"
        Write-Host "3. `$env:AURORA_VERSION_TAG = '$tag'; $(Format-Command $build)"
        Write-Host "4. $(Format-Command @('package-release.ps1', '-Exe', $exe, '-Mcp', $mcp, '-Tag', $tag, '-Destination', $dist, '-DumpSyms', $dumpSymsCommand.Source))"
        Write-Host "   $(Format-Command @('sign-release.ps1', '-Files', (Join-Path $dist "$name.zip.sha256"), '-KeyFile', $keyFile, '-PublicKey', ($updateKeys -join ',')))"
        Write-Host "5. $(Format-Command @('build-installer.ps1', '-Package', (Join-Path $dist $name), '-Tag', $tag, '-Destination', $dist, '-Iscc', $isccPath))"
        Write-Host '6. the release assets:'
        $assets | ForEach-Object { Write-Host "     $_" }
        Write-Host "   notes: $notes (folderstorm/release-notes-header.md$(if ($NotesFile) { " + $NotesFile" }), then GitHub's generated notes)"
        if ($Candidate) {
            Write-Host "7. $manifestPath   (version, tag, commit $($head.Substring(0, 8)), tree $($tree.Substring(0, 8)), each asset's SHA-256, build date); no draft"
        } else {
            Write-Host "7. $(if ($Draft) { '' } else { '(with -Draft) ' })$(Format-Command (@('gh') + $ghArgs))"
        }
        Write-Host ''
        Write-Host 'Dry run done: every check passed, nothing was changed.' -ForegroundColor Green
        return
    }

    $started = $true
    # --- 2. Tag --------------------------------------------------------------------
    if (-not ($FromExisting -and $localTag)) {
        Write-Step "Tag $tag (local only, never pushed$(if ($Candidate) { ', removed right after the build' }))"
        Invoke-Checked 'git tag' { git tag $tag }
        $tagged = $true
    }

    if (-not $FromExisting) {
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
            # a candidate's tag only told the build its version: main gets the real one
            if ($Candidate -and $tagged) {
                & git tag -d $tag *> $null
                if ($LASTEXITCODE -eq 0) { $tagged = $false; Write-Host "removed the local tag $tag" }
            }
        }

        # --- 4. Package: zip, symbols, checksums -------------------------------------
        Write-Step 'Package (zip, symbols, checksums)'
        $packaged = & (Join-Path $PSScriptRoot 'package-release.ps1') -Exe $exe -Mcp $mcp -Tag $tag -Destination $dist -DumpSyms $dumpSymsCommand.Source

        # --- 4b. Sign the zip's .sha256 for the updater -----------------------------
        # upstream's script: it refuses a key that is none of UPDATE_KEYS and
        # checks each signature it writes
        Write-Step 'Sign for the updater (<zip>.sha256.sig)'
        & (Join-Path $repo 'scripts/sign-release.ps1') -Files (Join-Path $dist "$name.zip.sha256") -KeyFile $keyFile -PublicKey $updateKeys | Out-Host
        $signature = Join-Path $dist "$name.zip.sha256.sig"
        if (-not (Test-Path -LiteralPath $signature -PathType Leaf) -or ((Get-Item -LiteralPath $signature).Length % 64) -ne 0) { Stop-Release "the signature $signature was not written" }

        # --- 5. Installer ------------------------------------------------------------
        Write-Step 'Installer (Inno Setup)'
        $installer = & (Join-Path $PSScriptRoot 'build-installer.ps1') -Package (Join-Path $dist $name) -Tag $tag -Destination $dist -Iscc $isccPath

        # every asset is there, and every checksum matches its file
        # only the paths the two scripts return: a stray blank or text line from a
        # tool's console output is not an asset
        $made = @($packaged) + @($installer) + @($signature) |
            Where-Object { $_ -is [string] -and $_.Trim() -and (Test-Path -LiteralPath $_ -PathType Leaf) } |
            ForEach-Object { (Resolve-Path -LiteralPath $_).Path }
        foreach ($asset in $assets) {
            if ($made -notcontains (Resolve-Path -LiteralPath $asset -ErrorAction SilentlyContinue).Path) { Stop-Release "missing release asset: $asset" }
        }
        Assert-Checksums $assets
    }
    # -FromExisting keeps the candidate's notes, unless -NotesFile gives new ones
    if (-not $FromExisting -or $NotesFile -or -not (Test-Path -LiteralPath $notes -PathType Leaf)) {
        $header = Get-Content -Raw -LiteralPath (Join-Path $repo 'folderstorm/release-notes-header.md')
        $text = $header.TrimEnd() + "`n"
        if ($NotesFile) { $text += "`n" + (Get-Content -Raw -LiteralPath $NotesFile).TrimEnd() + "`n" }
        [IO.File]::WriteAllText($notes, $text, [Text.UTF8Encoding]::new($false))
    }

    if ($Candidate) {
        # the build took a while: the manifest must name the commit it was built from
        if ((& git rev-parse HEAD).Trim() -ne $head) { Stop-Release "HEAD moved during the build (it was $($head.Substring(0, 8))): run again with -Clean" }
        $record = [ordered]@{
            schema  = 1
            version = $tag.Substring(1)
            tag     = $tag
            commit  = $head
            tree    = $tree
            branch  = "$branch"
            built   = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
            assets  = @($assets | ForEach-Object {
                    [ordered]@{ name = (Split-Path -Leaf $_); size = (Get-Item -LiteralPath $_).Length; sha256 = (Get-Sha256 $_) }
                })
        }
        [IO.File]::WriteAllText($manifestPath, ($record | ConvertTo-Json -Depth 4) + "`n", [Text.UTF8Encoding]::new($false))
    }

    # --- 6. Summary --------------------------------------------------------------------
    Write-Step $(if ($Candidate) { "Candidate $tag is built: these are the files to test" } elseif ($FromExisting) { "Release ${tag}: the tested files" } else { "Release $tag is built" })
    foreach ($file in $assets | Where-Object { -not $_.EndsWith('.sha256') -and -not $_.EndsWith('.sig') }) {
        $item = Get-Item -LiteralPath $file
        Write-Host ("{0}`n    {1:N1} MB   SHA-256 {2}" -f $item.FullName, ($item.Length / 1MB), (Get-Sha256 $file))
        Write-Host "  $($item.FullName).sha256"
        if (Test-Path -LiteralPath "$($item.FullName).sha256.sig") { Write-Host "  $($item.FullName).sha256.sig   (the updater's signature)" }
    }
    Write-Host ''
    Write-Host "The package folder $(Join-Path $dist $name) is not uploaded; try it: & '$(Join-Path (Join-Path $dist $name) (Get-BrandConstant 'EXE'))' --version"
    if ($Candidate) {
        Write-Host "The candidate's record: $manifestPath (commit $($head.Substring(0, 8)), tree $($tree.Substring(0, 8)))"
        Write-Host ''
        Write-Host 'Test these files (the setup, or the zip unpacked elsewhere); do not change them. Then merge the PR, with no other PR merged in between, and on main:' -ForegroundColor Green
        Write-Host "  $(Format-Command @('./folderstorm/scripts/build-release.ps1', '-Version', $tag, '-FromExisting', '-Draft'))"
        $built = $true
        return
    }
    Write-Host ''
    Write-Host 'The draft release (gh signed in by the human):'
    Write-Host "  $(Format-Command (@('gh') + $ghArgs))"
    $built = $true

    # --- 7. Draft ----------------------------------------------------------------------
    if ($Draft) {
        Write-Step 'Draft GitHub release'
        Invoke-Checked 'gh release create' { & gh @ghArgs }
        Write-Host ''
        Write-Host "Draft $tag created, not published. The human reviews it (title, notes, seven assets$(if ($isBeta) { ', pre-release box ticked' })) and clicks Publish release:" -ForegroundColor Green
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
    } elseif ($started -and $FromExisting) {
        Write-Host 'Nothing was built and the files are untouched: fix the cause, then run the same command again (it keeps a local tag already at HEAD).' -ForegroundColor Yellow
    } elseif ($started -and $Candidate) {
        if ($tagged) { & git tag -d $tag *> $null }
        Write-Host "Fix the cause, then run again with -Clean (it removes $dist first)." -ForegroundColor Yellow
    } elseif ($started) {
        Write-Host "Fix the cause, then run again with -Clean (it removes the local tag $tag and $dist first)." -ForegroundColor Yellow
    }
    exit 1
} finally {
    Pop-Location
}
