<#
.SYNOPSIS
    Folderstorm2's version of upstream's scripts/test-update.ps1: two test
    packages under the release names (Folderstorm2.exe, folderstorm-mcp.exe,
    folder and zip named after folderstorm-brand), the next one served on
    loopback, the first one started in an isolated installation. No tag, no
    release published (folderstorm/RELEASING.md, "Testing an update locally").

.DESCRIPTION
    Windows, from a git worktree, as upstream's script. Builds the dev
    profile twice with AURORA_UPDATE_TEST_VERSION (0.2.100, then 0.2.101),
    then restores the normal build. The server is upstream's
    scripts/update-test-server.ps1. Like a Folderstorm2 release, the feed
    offers no update policy file unless -Required, -ClearCache or
    -ResetSettings asks for one.
    -Beta serves the next version as a beta (a GitHub pre-release) and starts
    the isolated profile on the Beta channel.
#>
param([string]$Title = 'Test viewer-update', [Nullable[int]]$FpsLimit, [string]$Language = 'fr', [switch]$ClearCache, [switch]$ResetSettings, [switch]$Required, [switch]$Beta)
$ErrorActionPreference = 'Stop'
function Write-Utf8 {
    param([Parameter(ValueFromPipeline = $true)][string]$Text, [Parameter(Mandatory = $true)][string]$LiteralPath)
    begin { $lines = @() }
    process { $lines += $Text }
    end { [IO.File]::WriteAllText($LiteralPath, (($lines -join "`r`n") + "`r`n"), (New-Object Text.UTF8Encoding $false)) }
}
$repo = (git rev-parse --show-toplevel).Trim()
Set-Location -LiteralPath $repo
if (!(Test-Path -LiteralPath (Join-Path $repo '.git') -PathType Leaf)) { throw 'Run this test from a dedicated worktree.' }
# the shipped names come from folderstorm-brand
$brand = Get-Content -Raw -LiteralPath (Join-Path $repo 'crates/folderstorm-brand/src/lib.rs')
if ($brand -notmatch '(?m)^pub const RELEASE_PREFIX: &str = "([^"]+)";') { throw 'folderstorm-brand has no RELEASE_PREFIX' }
$prefix = $Matches[1]
if ($brand -notmatch '(?m)^pub const EXE: &str = "([^"]+)";') { throw 'folderstorm-brand has no EXE' }
$exeName = $Matches[1]

$root = Join-Path $repo ".scratch\update-test-$([Guid]::NewGuid().ToString('N'))"
$feed = Join-Path $root 'feed'
New-Item -ItemType Directory -Path $feed | Out-Null
$previousVersion = $env:AURORA_UPDATE_TEST_VERSION
$next = if ($Beta) { '0.2.101-beta.1' } else { '0.2.101' }
$folder = "$prefix-v$next-windows-x64"
try {
    foreach ($version in @('0.2.100', $next)) {
        $env:AURORA_UPDATE_TEST_VERSION = "v$version"
        cargo build --locked -p aurora-viewer -p folderstorm-mcp
        if ($LASTEXITCODE -ne 0) { throw "Build $version failed." }
        $out = if ($version -eq '0.2.100') { Join-Path $root 'install' } else { Join-Path $feed $folder }
        & "$PSScriptRoot/package-release.ps1" -Exe (Join-Path $repo 'target/debug/aurora-viewer.exe') -Mcp (Join-Path $repo 'target/debug/folderstorm-mcp.exe') -Out $out | Out-Null
    }
} finally {
    if ($null -eq $previousVersion) { Remove-Item Env:AURORA_UPDATE_TEST_VERSION -ErrorAction SilentlyContinue } else { $env:AURORA_UPDATE_TEST_VERSION = $previousVersion }
}
# Restore the normal Git version in the worktree's ordinary build.
cargo build --locked -p aurora-viewer -p folderstorm-mcp
if ($LASTEXITCODE -ne 0) { throw 'Failed to restore the normal build.' }

# The archive as a release carries it: entries under the folder, named with `/`.
$name = "$folder.zip"
$zip = Join-Path $feed $name
Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
$package = (Resolve-Path -LiteralPath (Join-Path $feed $folder)).Path
$archive = [IO.Compression.ZipFile]::Open($zip, [IO.Compression.ZipArchiveMode]::Create)
try {
    foreach ($file in Get-ChildItem -LiteralPath $package -Recurse -File) {
        $entry = $folder + '/' + $file.FullName.Substring($package.Length + 1).Replace('\', '/')
        [void][IO.Compression.ZipFileExtensions]::CreateEntryFromFile($archive, $file.FullName, $entry)
    }
} finally {
    $archive.Dispose()
}
$hash = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant()
"$hash  $name" | Write-Utf8 -LiteralPath "$zip.sha256"

$listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
$listener.Start(); $port = $listener.LocalEndpoint.Port; $listener.Stop()
$base = "http://127.0.0.1:$port"
@{ schema = 1; feed = "$base/releases.json" } | ConvertTo-Json | Write-Utf8 -LiteralPath (Join-Path $root 'aurora-update-test.json')
$assets = @(
    @{ name = $name; size = (Get-Item -LiteralPath $zip).Length; browser_download_url = "$base/$name" },
    @{ name = "$name.sha256"; size = (Get-Item -LiteralPath "$zip.sha256").Length; browser_download_url = "$base/$name.sha256" }
)
if ($Required -or $ClearCache -or $ResetSettings) {
    $policy = Join-Path $feed 'aurora-update-policy.json'
    # a real $null, not an empty `$(if …)` (ConvertTo-Json of Windows PowerShell 5.1 writes {})
    $minimum = if ($Required) { '0.2.101' } else { $null }
    $cache = if ($ClearCache) { '0.2.101' } else { $null }
    $settings = if ($ResetSettings) { '0.2.101' } else { $null }
    @{ schema = 1; version = $next; minimum_supported_version = $minimum; cache_invalidation_before = $cache; settings_reset_before = $settings } | ConvertTo-Json | Write-Utf8 -LiteralPath $policy
    $assets += @{ name = 'aurora-update-policy.json'; size = (Get-Item -LiteralPath $policy).Length; browser_download_url = "$base/aurora-update-policy.json" }
}
New-Item -ItemType Directory -Path (Join-Path $root 'profile'), (Join-Path $root 'cache') | Out-Null
@{ settings_version = 9; language = $Language; draw_distance = 300; update_channel = $(if ($Beta) { 'Beta' } else { 'Stable' }) } | ConvertTo-Json | Write-Utf8 -LiteralPath (Join-Path $root 'profile/settings.json')
'Isolated cache preservation proof' | Set-Content -LiteralPath (Join-Path $root 'cache/update-marker.txt')
'Isolated profile preservation proof' | Set-Content -LiteralPath (Join-Path $root 'profile/update-marker.txt')
ConvertTo-Json -InputObject @(@{ tag_name = "v$next"; draft = $false; prerelease = [bool]$Beta; html_url = "$base/release.html"; body = 'Trial of the real installation in an isolated folder.'; assets = $assets }) -Depth 6 | Write-Utf8 -LiteralPath (Join-Path $feed 'releases.json')
"<!doctype html><meta charset=`"utf-8`"><title>Test release</title><p>Local test release $next.</p>" | Write-Utf8 -LiteralPath (Join-Path $feed 'release.html')

$server = Start-Process -FilePath (Get-Process -Id $PID).Path -ArgumentList @('-NoProfile', '-File', "`"$repo/scripts/update-test-server.ps1`"", '-Root', "`"$root`"", '-Port', $port) -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $root 'server.stdout.log') -RedirectStandardError (Join-Path $root 'server.stderr.log')
for ($i = 0; $i -lt 100 -and !(Test-Path -LiteralPath (Join-Path $root 'server-ready')); $i++) { if ($server.HasExited) { throw 'Test server stopped.' }; Start-Sleep -Milliseconds 100 }
if (!(Test-Path -LiteralPath (Join-Path $root 'server-ready'))) { throw 'Test server unavailable.' }
$previousRoot = $env:AURORA_UPDATE_TEST_ROOT
$previousDemo = $env:AURORA_DEMO
try {
    $env:AURORA_UPDATE_TEST_ROOT = $root
    Remove-Item Env:AURORA_DEMO -ErrorAction SilentlyContinue
    $arguments = @('--title', "`"$Title`"", '--language', $Language)
    if ($null -ne $FpsLimit) { $arguments += @('--fps-limit', $FpsLimit) }
    $install = Join-Path $root 'install'
    $viewer = Start-Process -FilePath (Join-Path $install $exeName) -WorkingDirectory $install -ArgumentList $arguments -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $root 'viewer.stdout.log') -RedirectStandardError (Join-Path $root 'viewer.stderr.log')
} finally {
    if ($null -eq $previousRoot) { Remove-Item Env:AURORA_UPDATE_TEST_ROOT -ErrorAction SilentlyContinue } else { $env:AURORA_UPDATE_TEST_ROOT = $previousRoot }
    if ($null -eq $previousDemo) { Remove-Item Env:AURORA_DEMO -ErrorAction SilentlyContinue } else { $env:AURORA_DEMO = $previousDemo }
}
@{ root = $root; server_pid = $server.Id; viewer_pid = $viewer.Id } | ConvertTo-Json | Write-Utf8 -LiteralPath (Join-Path $repo '.scratch/update-test-current.json')
Write-Output "Test installation: $root\install"
Write-Output "Viewer: $($viewer.Id); local server: $($server.Id) (stop it when done: Stop-Process -Id $($server.Id))"
