# Releasing Folderstorm2

How to make a GitHub release of Folderstorm2 that installed viewers take as
an update. Written so that an agent can follow it step by step on the
human's Windows PC. The background is in
[plans/releases-and-installer.md](plans/releases-and-installer.md), sections
1, 2, 3 and 8.

There are three ways, with the same result (a **draft** release with the
same six assets):

- **One script** (the usual way): `folderstorm/scripts/build-release.ps1` on
  the human's Windows PC ([below](#the-usual-way-one-script)). To release
  the very files tested on a PR branch, without building again on `main`:
  [Test, then release the same files](#test-then-release-the-same-files).
- **By hand** (the fallback, if the script cannot be used): the same steps
  one at a time ([below](#by-hand-the-fallback)).
- **GitHub Actions**: Actions tab › **Folderstorm release build** › Run
  workflow, with the version filled in (see [CI.md](CI.md)). It runs the same
  packaging and installer scripts.

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

## Versions

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

The human chooses the version.

## The release assets

Six files, in `target/release-assets/<tag>/` (ignored by git; under
`$env:CARGO_TARGET_DIR` instead of `target/` when it is set). For `v0.1.0`:

| File | What |
|---|---|
| `Folderstorm2-v0.1.0-windows-x64.zip` | the update and download: `Folderstorm2.exe`, `folderstorm-mcp.exe`, `assets/`, `LICENSE`, `NOTICE.md`, `README.md`, `aurora-install.json`, all inside one folder of the same name |
| `Folderstorm2-v0.1.0-windows-x64.zip.sha256` | `<sha256>  <zip name>`, the line the updater checks |
| `Folderstorm2-v0.1.0-windows-x64-symbols.zip` | Breakpad symbols for crash reports |
| `Folderstorm2-v0.1.0-windows-x64-symbols.zip.sha256` | its checksum |
| `Folderstorm2-v0.1.0-windows-x64-setup.exe` | the per-user installer (Inno Setup): the same files as the zip, in `%LOCALAPPDATA%\Programs\Folderstorm2`, with Start menu shortcuts. The updater ignores it |
| `Folderstorm2-v0.1.0-windows-x64-setup.exe.sha256` | its checksum |

Beside them, not uploaded: the package folder `Folderstorm2-v0.1.0-windows-x64/`
(the zip's content, for a quick try) and `release-notes.md` (the notes the
draft starts with).

## Once per PC

The human installs these, once:

| Tool | Install | Needed for |
|---|---|---|
| Rust | <https://rustup.rs> | the build |
| `dump_syms` | `cargo install dump_syms --locked --version 2.3.9` (the version upstream's release uses) | the symbols |
| Inno Setup 6 | `winget install --id JRSoftware.InnoSetup -e` (or `choco install innosetup`) | the installer |
| GitHub CLI | `winget install --id GitHub.cli -e`, then the human runs `gh auth login` | the draft (`-Draft`) |
| PowerShell 7 | `winget install --id Microsoft.PowerShell -e` | the scripts: run them from `pwsh`, not Windows PowerShell 5.1 |

## The usual way: one script

From the repository root, on `main`, with a clean working tree that matches
`origin/main` (`git switch main`, `git pull --ff-only origin main`):

```powershell
./folderstorm/scripts/build-release.ps1 -Version 0.1.0 -DryRun   # checks only, changes nothing
./folderstorm/scripts/build-release.ps1 -Version 0.1.0 -Draft    # the release and its draft
```

A beta: `-Version 0.2.0 -Beta 1` (or `-Version v0.2.0-beta.1`). The human's
own notes, put after `folderstorm/release-notes-header.md` and before
GitHub's generated notes: `-NotesFile <file>`.

In order, it stops with a clear message at the first problem:

1. **Checks**: on `main`, clean, up to date with `origin/main`; the version
   is a release version (section [Versions](#versions)); the tag exists
   neither here nor on `origin`, and is higher than every release there;
   `cargo`, `dump_syms` and Inno Setup's `ISCC.exe` are installed (it looks
   on `PATH` and in the default install folders; `-Iscc <path>` names it);
   with `-Draft`, `gh` is installed and signed in, and GitHub has no release
   or draft of that tag yet.
2. **Tag**: `git tag <tag>`, local only (the build only takes the version
   from `AURORA_VERSION_TAG` when the checked-out commit **is** that tag,
   `crates/aurora-version`).
3. **Build**: `cargo build --release --locked -p aurora-viewer -p
   folderstorm-mcp --features aurora-viewer/xr-static-loader`, with
   `AURORA_VERSION_TAG` set. The release profile, not `--profile ci`: this
   build ships. It keeps what the human's build skill set in the
   environment (for example the chat image host's
   `FOLDERSTORM_IMAGE_HOST_FOLDER`); never type the optional
   `FOLDERSTORM_IMAGE_HOST_TOKEN` yourself.
4. **Package**: `folderstorm/scripts/package-release.ps1 -Tag`, which checks
   that `--version` prints `Folderstorm2 <version>` and makes the zip, the
   symbols and their checksums.
5. **Installer**: `folderstorm/scripts/build-installer.ps1`, which compiles
   `folderstorm/installer/folderstorm2.iss` from the package folder and
   writes the setup's checksum.
6. **Summary**: every asset with its size and SHA-256, and the exact
   `gh release create` command.
7. **Draft** (with `-Draft`): that command, `gh release create <tag> --draft
   --target <commit>` (`--prerelease` for a beta) with the six assets and
   the notes. Never published, never `--latest`.

**After a failure**, fix the cause and run the same command again with
`-Clean`: it removes the local tag and `target/release-assets/<tag>/` first.
If only the draft step failed, the files are built: paste the printed `gh`
command instead.

Then give the human the draft's link (`gh release view <tag> --repo
Sodie-Pop/Folderstorm2 --web` opens it) and stop: see
[The human reviews and publishes](#the-human-reviews-and-publishes).

## Test, then release the same files

To release exactly the files the human tested on a PR branch, with one
build instead of two:

1. **On the PR branch** (clean, and up to date with `main`):
   `./folderstorm/scripts/build-release.ps1 -Version 0.2.0 -Candidate`.
   The full release build (version stamped, zip, symbols, installer,
   checksums), plus `release-candidate.json`: version, tag, commit, tree
   hash, each asset's SHA-256, build date. The local tag only serves the
   build and is removed right after it; nothing is pushed; no draft.
2. **The human tests** the files it lists, in the **main checkout's**
   `target/release-assets/<tag>/` (`$env:CARGO_TARGET_DIR`'s, when set),
   even when the branch is a task worktree: `end-task.ps1` and
   `new-task.ps1` delete a worktree's `target/`. Install the setup or unzip
   the zip elsewhere; never change the files.
3. **Merge the PR, and let no other PR merge before step 4.**
4. **On `main`** (`git switch main`, `git pull --ff-only origin main`):
   `./folderstorm/scripts/build-release.ps1 -Version 0.2.0 -FromExisting -Draft`
   (`-DryRun` to check only). No cargo, no build. Besides the usual checks
   (clean, up to date, the tag new and higher than the last release, `gh`),
   it refuses unless `main`'s tree (`git rev-parse HEAD^{tree}`) is the
   candidate's tree, which means the merged code is byte for byte what was
   built, and unless every asset matches the recorded SHA-256 and its
   `.sha256` file. Then it makes the local tag on `main` and the draft
   (`--target` is `main`'s HEAD) with those files. The notes are the
   candidate's, or a new `-NotesFile`'s.
5. **The human publishes** the draft
   ([below](#the-human-reviews-and-publishes)).

A merge commit, a squash or a rebase of a PR that was up to date with `main`
has the PR's tree. If the trees differ (another PR merged in between, the
branch was behind `main`, or a commit came after the candidate), it lists
the changed files (`git diff --stat <candidate commit> HEAD`) and releases
nothing: build and test a new candidate (`-Candidate -Clean`), or release
from `main` the usual way, which builds again. `-Candidate` warns when the
branch does not contain `origin/main`. `-Clean` does not go with
`-FromExisting`: a local tag already at `main`'s HEAD is kept, any other
one must be removed by hand (`git tag -d <tag>`).

**The built-in commit**: Help › About, bug reports, crash reports and the
dev toolbox show the commit the binary was built from
(`aurora_version::commit()`): with a candidate, the PR's commit, not
`main`'s merge commit. The version itself has no commit in it (`0.2.0`).
That is expected: the two commits have the same tree.

## By hand (the fallback)

The same steps one at a time, if the script cannot be used. Below, `$Tag`
is the version:

```powershell
$Tag = 'v0.1.0'            # or 'v0.2.0-beta.1'
if ($Tag -notmatch '^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-beta\.(0|[1-9]\d*))?$') { throw "Not a release version: $Tag" }
```

### 1. Tag

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

### 2. Build

```powershell
$env:AURORA_VERSION_TAG = $Tag
cargo build --release --locked -p aurora-viewer -p folderstorm-mcp --features aurora-viewer/xr-static-loader
Remove-Item Env:AURORA_VERSION_TAG
```

It also writes `target/release/aurora_viewer.pdb`, which the symbols need.
(If `CARGO_TARGET_DIR` is set, the files are under that folder instead: use
it in the paths below.)

### 3. Package and installer

```powershell
$Dist = "target/release-assets/$Tag"
$Name = "Folderstorm2-$Tag-windows-x64"
./folderstorm/scripts/package-release.ps1 -Exe target/release/aurora-viewer.exe -Mcp target/release/folderstorm-mcp.exe -Tag $Tag
./folderstorm/scripts/build-installer.ps1 -Package "$Dist/$Name" -Tag $Tag
```

### 4. Verify

```powershell
& "$Dist/$Name/Folderstorm2.exe" --version          # Folderstorm2 0.1.0
foreach ($file in "$Dist/$Name.zip", "$Dist/$Name-symbols.zip", "$Dist/$Name-setup.exe") {
    $line = (Get-Content -Raw -LiteralPath "$file.sha256").Trim()
    $hash = (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($line -cne "$hash  $(Split-Path -Leaf $file)") { throw "Checksum mismatch: $file" }
}
```

### 5. Create the draft release

`gh` must already be signed in by the human (`gh auth status`):

```powershell
$assets = "$Dist/$Name.zip", "$Dist/$Name.zip.sha256", "$Dist/$Name-symbols.zip", "$Dist/$Name-symbols.zip.sha256",
    "$Dist/$Name-setup.exe", "$Dist/$Name-setup.exe.sha256"
$ghArgs = @($Tag) + $assets + @('--repo', 'Sodie-Pop/Folderstorm2', '--draft', '--target', $Commit,
    '--title', "Folderstorm2 $Tag", '--notes-file', 'folderstorm/release-notes-header.md', '--generate-notes')
if ($Tag -match '-') { $ghArgs += '--prerelease' }   # v0.2.0-beta.1
gh release create @ghArgs
```

`--target` is the tagged commit: GitHub creates the tag there when the draft
is published. Add the human's own notes with `--notes-file` of a file that
starts with `folderstorm/release-notes-header.md`, if they wrote some.

## The human reviews and publishes

The human checks the draft on GitHub (title, notes, the six assets, the
pre-release box for a beta), then clicks **Publish release**. Installed
viewers see it within ten minutes.

Afterwards, `git fetch origin --tags` replaces the local tag with GitHub's
(same commit).

**Abandoning a draft**: delete it on GitHub (or `gh release delete <tag>
--repo Sodie-Pop/Folderstorm2`, asked by the human), then `git tag -d
<tag>` (or run the script again with `-Clean`).

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
`v0.2.0-beta.1`, install it with its setup, set the channel to Beta, publish
`v0.2.0-beta.2`, and see the installed copy update itself (the Start menu
shortcuts keep working, see [Testing the installer](#testing-the-installer)).

## Testing the installer

On Windows, with the setup of a release built as above (the draft need not
exist), as an ordinary user (no admin prompt should appear):

1. **Install**: run the setup. It offers the folder
   `%LOCALAPPDATA%\Programs\Folderstorm2` and three unticked tasks (a desktop
   shortcut for the viewer, one for the session host, and `secondlife://`
   links). Start menu › Folderstorm2: **Folderstorm2** opens the viewer,
   **Folderstorm2 Session Host** opens the characters' host
   (`--session-host`).
2. **Links** (optional): run the setup again with the links task ticked.
   Preferences › Interface then says the links open this viewer; a
   `secondlife://` link in a browser opens it.
3. **Update**: install an older version's setup (or two betas), then let the
   viewer update itself, or test with `test-update.ps1`'s packages. After the
   update, both Start menu shortcuts still start the new version
   (`--version`, or Help › About), and Settings › Apps still lists
   Folderstorm2 (its version there stays the installed one until the next
   setup: cosmetic).
4. **Uninstall, keeping data**: Settings › Apps › Folderstorm2 › Uninstall,
   answer **No** to "Also remove your settings…". The folder
   `%LOCALAPPDATA%\Programs\Folderstorm2`, the shortcuts, the link classes
   that opened it and any `.aurora-update-*` folder or
   `.Folderstorm2.update.lock` it left in `%LOCALAPPDATA%\Programs` are gone;
   `%APPDATA%\Folderstorm\Folderstorm2` and
   `%LOCALAPPDATA%\Folderstorm\Folderstorm2` stay. Reinstall: the saved
   logins are still there.
5. **Uninstall, removing data**: the same, answer **Yes**. The viewer's own
   removal runs first (`Folderstorm2.exe --uninstall`: saved passwords,
   second-factor tokens, cache key, link classes, notification identity,
   Blender add-on), then both data folders go. Folders chosen in Preferences
   (another cache location, conversation history) stay.
