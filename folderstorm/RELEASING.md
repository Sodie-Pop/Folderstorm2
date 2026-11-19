# Releasing Folderstorm2

How to make a GitHub release of Folderstorm2 that installed viewers take as
an update. Written so that an agent can follow it step by step on the
human's Windows PC. The background is in
[plans/releases-and-installer.md](plans/releases-and-installer.md), sections
1, 2 and 8.

There are two ways, with the same result (a **draft** release with the same
assets):

- **Local** (the usual way): the steps below, on the human's Windows PC.
- **GitHub Actions**: Actions tab › **Folderstorm release build** › Run
  workflow, with the version filled in (see [CI.md](CI.md)). It runs the same
  packaging script, then step 7 is the same.

Either way, **nothing reaches users until the human publishes the draft**:
viewers ignore drafts.

## Rules

- **The human signs `gh` in, once, on that PC** (`gh auth login`). Agents
  never type, read, store or pass a GitHub token, password or key, never set
  `GH_TOKEN` or `GITHUB_TOKEN`, and never run `gh auth` commands that print a
  token. If `gh auth status` says it is not signed in, stop and ask the
  human.
- **Agents never publish.** They create a draft; the human reads it and
  clicks **Publish release**.
- **Never push a tag.** The local tag only tells the build its version.
  GitHub creates the real tag when the human publishes the draft.
- Never push Aurora's tags (`upstream`'s) to this repository: development
  builds would then read Aurora's version numbers.
- No `aurora-update-policy.json` in the release (decided: every update is
  optional for now). Never attach upstream's `update-policy.json`.

## 1. Check the version

Folderstorm2 has its own [SemVer](https://semver.org) line, separate from
Aurora's:

| Kind | Tag | GitHub "pre-release" flag | Who gets it |
|---|---|---|---|
| Stable | `v0.1.0`, `v0.1.1`, `v0.2.0` | **off** | everyone (Stable channel, the default) |
| Beta | `v0.2.0-beta.1`, `v0.2.0-beta.2` | **on** | the Beta channel (Preferences › Language and updates) |

- The first release is **`v0.1.0`**.
- A beta's pre-release starts with the word `beta`, then a number:
  `-beta.N`. Other words (`-rc.1`, `-alpha`) are ignored by both channels.
- The flag must match the tag, or viewers ignore the release.
- The version must be **higher** than the last published one (SemVer order:
  `0.2.0-beta.2` < `0.2.0` < `0.2.1`). See
  <https://github.com/Sodie-Pop/Folderstorm2/releases>, or
  `gh release list --repo Sodie-Pop/Folderstorm2`.
- No build metadata (`+…`): that is a development build, never a release.

The human chooses the version. Below, `$Tag` stands for it:

```powershell
$Tag = 'v0.1.0'            # or 'v0.2.0-beta.1'
if ($Tag -notmatch '^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-beta\.(0|[1-9]\d*))?$') { throw "Not a release version: $Tag" }
```

## 2. Tag

From an up-to-date `main`, with a clean working tree:

```powershell
git switch main
git pull --ff-only origin main
git status --porcelain     # must print nothing
git fetch origin --tags
if (git tag --list $Tag) { throw "$Tag already exists" }
git tag $Tag               # local only: never push it
$Commit = git rev-parse HEAD
```

The build only takes the version from `AURORA_VERSION_TAG` when the
checked-out commit **is** that tag (`crates/aurora-version`), so tag first.

## 3. Build

In the same PowerShell window (the human's build skill may already set the
chat image host's `FOLDERSTORM_IMAGE_HOST_FOLDER`; keep what it sets, and
never type the optional `FOLDERSTORM_IMAGE_HOST_TOKEN` yourself):

```powershell
$env:AURORA_VERSION_TAG = $Tag
cargo build --release --locked -p aurora-viewer -p folderstorm-mcp --features aurora-viewer/xr-static-loader
Remove-Item Env:AURORA_VERSION_TAG
```

The release profile, not `--profile ci`: this build ships. It also writes
`target/release/aurora_viewer.pdb`, which the symbols need. (If
`CARGO_TARGET_DIR` is set, the files are under that folder instead: use it in
the paths below.)

Once per PC, install the symbol tool, the version upstream's release uses:

```powershell
cargo install dump_syms --locked --version 2.3.9
```

## 4. Package

```powershell
./folderstorm/scripts/package-release.ps1 -Exe target/release/aurora-viewer.exe -Mcp target/release/folderstorm-mcp.exe -Tag $Tag
```

It checks that `--version` prints `Folderstorm2 <version>` (the tag
without `v`), then writes to `target/release-assets/<tag>/` (ignored by git):

| File | What |
|---|---|
| `Folderstorm2-<tag>-windows-x64/` | the package folder (not uploaded) |
| `Folderstorm2-<tag>-windows-x64.zip` | the update and download: `Folderstorm2.exe`, `folderstorm-mcp.exe`, `assets/`, `LICENSE`, `NOTICE.md`, `README.md`, `aurora-install.json`, all inside that one folder |
| `Folderstorm2-<tag>-windows-x64.zip.sha256` | `<sha256>  <zip name>`, the line the updater checks |
| `Folderstorm2-<tag>-windows-x64-symbols.zip` | Breakpad symbols for crash reports |
| `Folderstorm2-<tag>-windows-x64-symbols.zip.sha256` | its checksum |

The script prints the four asset paths. For `v0.1.0`:
`Folderstorm2-v0.1.0-windows-x64.zip`, `Folderstorm2-v0.1.0-windows-x64.zip.sha256`,
`Folderstorm2-v0.1.0-windows-x64-symbols.zip`,
`Folderstorm2-v0.1.0-windows-x64-symbols.zip.sha256`.

## 5. Verify

```powershell
$Dist = "target/release-assets/$Tag"
$Name = "Folderstorm2-$Tag-windows-x64"
& "$Dist/$Name/Folderstorm2.exe" --version          # Folderstorm2 0.1.0
foreach ($zip in "$Dist/$Name.zip", "$Dist/$Name-symbols.zip") {
    $line = (Get-Content -Raw -LiteralPath "$zip.sha256").Trim()
    $hash = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($line -cne "$hash  $(Split-Path -Leaf $zip)") { throw "Checksum mismatch: $zip" }
}
```

Optional, before a first release or after a change to the updater: test an
update between two local packages ([below](#testing-an-update-locally)).

## 6. Create the draft release

`gh` must already be signed in by the human (`gh auth status`):

```powershell
$assets = "$Dist/$Name.zip", "$Dist/$Name.zip.sha256", "$Dist/$Name-symbols.zip", "$Dist/$Name-symbols.zip.sha256"
$ghArgs = @($Tag) + $assets + @('--repo', 'Sodie-Pop/Folderstorm2', '--draft', '--target', $Commit,
    '--title', "Folderstorm2 $Tag", '--notes-file', 'folderstorm/release-notes-header.md', '--generate-notes')
if ($Tag -match '-') { $ghArgs += '--prerelease' }   # v0.2.0-beta.1
gh release create @ghArgs
```

`--target` is the tagged commit: GitHub creates the tag there when the draft
is published. Add the human's own notes with `--notes-file` of a file that
starts with `folderstorm/release-notes-header.md`, if they wrote some.

Then give the human the draft's link (`gh release view $Tag --repo
Sodie-Pop/Folderstorm2 --web` opens it) and stop.

## 7. The human reviews and publishes

The human checks the draft on GitHub (title, notes, the four assets, the
pre-release box for a beta), then clicks **Publish release**. Installed
viewers see it within ten minutes.

Afterwards, `git fetch origin --tags` replaces the local tag with GitHub's
(same commit).

**Abandoning a draft**: delete it on GitHub (or `gh release delete $Tag
--repo Sodie-Pop/Folderstorm2`, asked by the human), then `git tag -d $Tag`.

**A published release with a problem**: never replace its assets, since
viewers may already have the old checksum. Publish a higher version instead.

## Testing an update locally

`folderstorm/scripts/test-update.ps1` (Windows, from a git worktree, as
upstream's `scripts/test-update.ps1`) builds two dev packages, `0.2.100` and
`0.2.101`, under the release names, serves the second from a loopback server
and starts the first in an isolated folder (its own settings, cache and
logs; login disabled). In the viewer: the update offer appears; Install
restarts into `0.2.101`. Check that the installation folder then holds the
new `Folderstorm2.exe` and `folderstorm-mcp.exe`. `-Beta` serves a beta
instead. Nothing is published and no tag is made.

After a first real release, the full check is a pair of betas: publish
`v0.2.0-beta.1`, install it from its zip, set the channel to Beta, publish
`v0.2.0-beta.2`, and see the first update itself.
