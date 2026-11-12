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
| macOS (`macos.yml`) | **Keep** | clippy and tests on macOS, only when a change touches macOS code (it ends in seconds otherwise). Uses GitHub's runners here too. |
| Folderstorm tests (`folderstorm-tests.yml`) | **Keep** | Ours: Windows tests on every pull request (see below). |
| Nightly (`nightly.yml`) | Disable | Tags and publishes Aurora nightly builds twice a day. |
| Fuzz (`fuzz.yml`) | Disable | Nightly fuzzing of upstream's parsers. Upstream already runs it. |
| PR description (`pr-description.yml`) | Disable | Upstream's bot that enforces its PR template and labels. |
| Review slots (`pr-slots.yml`) | Disable | Upstream's limit on outside contributors' PRs. |
| Crash triage (`crash-triage.yml`) | Disable | Sorts crash reports filed as issues on Aurora's repository. |
| Wiki (`wiki.yml`) | Disable | Publishes upstream's guides to the repository wiki. |
| Release (`release.yml`) | Disable (for now) | Builds and publishes "Aurora-Viewer" zips on a version tag. Re-enable, adapted, with the branding task (FEATURES.md 0.1 / 0.3). |
| Folderstorm release build (`folderstorm-release.yml`) | **Keep** | Ours: manual build artifact (see below). |
| Media plugins (`media-plugins.yml`) | Disable | Manual only. Builds the media plugin packs. Aurora's published packs can be used instead. |

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
release mode and keeps the result as a workflow artifact. It runs only when
started by hand, publishes nothing (no tag, no GitHub release), signs nothing
and does not touch the update feed: those come later (FEATURES.md 0.3).

**Run it:** Actions tab › **Folderstorm release build** › **Run workflow**.
Inputs: an optional version label (used in file names only; the version inside
the viewer stays the one Git describes) and the platforms (Windows only by
default; "windows and linux" adds a Linux job). The artifact
`<slug>-<label>-<commit>-windows-x64` holds a zip and its `.sha256`: the two
executables, `aurora-install.json`, `LICENSE`, `NOTICE.md`, `README.md` and the
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
