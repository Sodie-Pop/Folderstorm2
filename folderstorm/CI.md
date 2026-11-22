# CI on the Folderstorm2 fork

Upstream's workflow files stay **unchanged**, so that upstream merges never
conflict on them. Each workflow that only makes sense for the Aurora team is
turned off in GitHub's settings instead of being edited. GitHub keeps that
setting when the file changes in a later merge.

## Turn Actions on, then disable the upstream-only workflows

GitHub turns Actions off on a fork until its owner enables them.

1. On GitHub, open the repository's **Actions** tab and click
   **I understand my workflows, go ahead and enable them**.
2. For each workflow marked "Disable" below: click it in the left list, open
   the **⋯** menu (top right) and choose **Disable workflow**.

| Workflow (file) | Keep? | Why |
|---|---|---|
| CI (`ci.yml`) | **Keep** | fmt, clippy (Windows and Linux), secret scan. Tests run on pushes to `main`. Without Aurora's private runners and token, it uses GitHub's runners. |
| macOS (`macos.yml`) | Disable | clippy and tests on macOS when a change touches macOS code. Since 2026-11-16 GitHub's `macos-15` runners never start for the fork (jobs wait 15 minutes, then are cancelled without a log), and no Mac build ships, so the human disabled it in the Actions tab. The file stays as upstream's; enable it again there if a Mac build is ever wanted. |
| Folderstorm tests (`folderstorm-tests.yml`) | **Keep** | Ours: Windows tests on every pull request (see below). |
| Nightly (`nightly.yml`) | Disable | Tags and publishes Aurora nightly builds twice a day. |
| Fuzz (`fuzz.yml`) | Disable | Nightly fuzzing of upstream's parsers. Upstream already runs it. |
| PR description (`pr-description.yml`) | Disable | Upstream's bot that enforces its PR template and labels. |
| Review slots (`pr-slots.yml`) | Disable | Upstream's limit on outside contributors' PRs. |
| Crash triage (`crash-triage.yml`) | Disable | Sorts crash reports filed as issues on Aurora's repository. |
| Wiki (`wiki.yml`) | Disable | Publishes upstream's guides to the repository wiki. |
| Release (`release.yml`) | Disable | Builds and publishes "Aurora-Viewer" zips on a version tag, with Aurora's update policy. The fork releases with the local script of [RELEASING.md](RELEASING.md) or `folderstorm-release.yml` instead. |
| Sign test (`sign-test.yml`) | Disable | Manual only. Tries upstream's SignPath code signing with Aurora's SignPath project; it fails without the `SIGNPATH_*` variable and secret, which the fork does not have. Release and Nightly (already disabled) also sign through SignPath now (`.github/actions/signpath-sign`). |
| Folderstorm release build (`folderstorm-release.yml`) | **Keep** | Ours: a draft GitHub release from a version, or a test build artifact (see below). |
| Media plugins (`media-plugins.yml`) | Disable | Manual only. Builds the media plugin packs. Aurora's published packs can be used instead. |
| FFmpeg pack (`ffmpeg-pack.yml`) | Disable | Builds the Director's ffmpeg pack, on a pull request that changes `scripts/ffmpeg-pack.sh` or by hand, with macOS jobs that never start on the fork. The viewer downloads Aurora's published pack instead (see BRANDING.md). |

The repository is public, so GitHub-hosted runners cost nothing.

Dependabot (`.github/dependabot.yml`) stays off on a fork by default. Leave
it off: dependency updates arrive with each upstream merge.

## Tests on pull requests

Upstream's `ci.yml` runs the tests only in its merge queue and on `main`. A
merge queue is only offered to repositories owned by an organization. So
`folderstorm-tests.yml` runs the same three Windows test parts on every pull
request, and skips them when only documentation changed. Once it has run
once, it can be required in **Settings → Branches → branch protection rule
for `main`** through its check **tests (Windows, PR)**, alongside CI's
**fmt, clippy (Windows)**, **clippy (Linux)** and **secret scan**.

## Manual release build

`folderstorm-release.yml` builds `aurora-viewer` and `folderstorm-mcp` in
release mode. It runs only when started by hand: Actions tab › **Folderstorm
release build** › **Run workflow**. It signs nothing (code signing comes
later: a commented hook waits in the `.iss`). Two modes:

**Draft release** (the `version` input filled in, for example `0.1.0` or
`0.2.0-beta.1`, run from `main`): the same result as the local script of
[RELEASING.md](RELEASING.md) (`folderstorm/scripts/build-release.ps1`), which
is the usual way. The build job (read-only
token) checks the version, tags the commit **locally** so that the build takes
the version (`AURORA_VERSION_TAG`), builds, installs `dump_syms` and runs
`folderstorm/scripts/package-release.ps1 -Tag`, which checks `--version` and
makes `Folderstorm2-v<V>-windows-x64.zip`, its `.sha256`,
`Folderstorm2-v<V>-windows-x64-symbols.zip` and its `.sha256`. Then
`folderstorm/scripts/build-installer.ps1` compiles the per-user installer
(`folderstorm/installer/folderstorm2.iss`, Inno Setup) from the same package
folder: `Folderstorm2-v<V>-windows-x64-setup.exe` and its `.sha256`. Inno
Setup 6 is on GitHub's `windows-latest` image (its software list, checked
2026-11-20); the step installs it with Chocolatey if it is ever missing.
The **Draft release** job, the only one with `contents: write`, then runs
`gh release create v<V> --draft --target <commit>` with those six files,
`folderstorm/release-notes-header.md` and GitHub's generated notes, and
`--prerelease` for a beta. Nothing is visible to viewers until the human
publishes the draft; GitHub creates the tag then. No update policy file is
attached (every update is optional), and never upstream's
`update-policy.json`. Linux is not built in this mode.

**Test build** (no version): the artifact is kept by the workflow, nothing is
published. Inputs: an optional version label (used in file names only; the
version inside the viewer stays the one Git describes) and the platforms
(Windows only by default; "windows and linux" adds a Linux job). The artifact
`<slug>-<label>-<commit>-windows-x64` holds a zip and its `.sha256`: the two
executables (the viewer renamed `Folderstorm2.exe`, see BRANDING.md, and
`folderstorm-mcp.exe`), `aurora-install.json`, `LICENSE`, `NOTICE.md`, `README.md` and the
licences of the bundled emoji font and spelling dictionaries (the files of
upstream's `scripts/package.ps1`, through `folderstorm/scripts/package-release.ps1`,
which accepts Folderstorm2's `--version` output). The slug comes from
`folderstorm-brand`.

**Chat image upload settings:** the build reads `FOLDERSTORM_IMAGE_HOST_FOLDER`
from the repository variable of the same name (Settings › Secrets and variables
› Actions › **Variables**), and the optional `FOLDERSTORM_IMAGE_HOST_TOKEN`
from a secret. Both are set in the build step's environment only. If the
variable is empty, the build still succeeds and the job summary warns that
uploads will need the folder entered in Preferences.

**Anything compiled into a release can be extracted from it.** A folder id is
fine; do not put in a secret that must stay private. A token compiled into the
viewer is public to everyone who downloads the build.

## At each upstream sync

List the workflow files the merge adds or changes:

```bash
git diff --stat HEAD~1 HEAD -- .github/
```

A **new** upstream workflow runs on the fork as soon as it is merged. Decide
whether to keep it, and disable it on GitHub if not. Note it in the table
above.
