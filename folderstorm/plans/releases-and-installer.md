# Releases, updates, installer and crash reports

Plan for FEATURES.md 0.3 (release checks and update feed) and 0.4 (crash
reports), with the Windows installer. **No code changes yet: the human
decides from this.** Written 2026-11-19 against `main` at `a624004f`.
**Decided 2026-11-19: see [section 8](#8-decided).**

**Status 2026-11-19:** PR 1 built on branch `claude/release-exe-name`, PR 2
on `claude/release-draft` (stacked), both on the upstream sync of
2026-11-19. PR 0 (upstream) skipped: the upstream tests were edited instead,
through one renaming helper in `tests/updates.rs`. PR 2 attaches no policy
file (decision 2), so no `folderstorm/update-policy.json` yet; the local
path is [RELEASING.md](../RELEASING.md). The upstream sync also brought
Linux in-place updates: the Linux package's folder and archive names follow
`RELEASE_PREFIX` too, its executable keeps cargo's name.

Line numbers point at that commit. "Upstream file" means a file of Aurora
Viewer, where we only add small `// Folderstorm:` hooks.

Contents:

1. [Program name at packaging](#1-program-name-at-packaging)
2. [Updates from GitHub releases](#2-updates-from-github-releases)
3. [Installer](#3-installer)
4. [Crash reports](#4-crash-reports)
5. [Bug-report links](#5-bug-report-links)
6. [Phases and PRs](#6-phases-and-prs)
7. [Questions for the human](#7-questions-for-the-human)
8. [Decided](#8-decided)

---

## Findings that change the plan

Read these first. Each one would break the first public release if it were
missed.

| # | Finding | Where | Effect if not fixed |
|---|---|---|---|
| F1 | The updater only accepts the executable name `aurora-viewer.exe`. | `aurora-updater/src/install.rs:14`, `:142`, `:259`, `package.rs:170` | A shipped `Folderstorm2.exe` can never update itself ("updater is not running from its installation"). |
| F2 | The updater refuses any package entry outside a fixed list, and `folderstorm-mcp.exe` is not in it. Our package script ships it. | `install.rs:58-66`, `:160-175`; `folderstorm/scripts/package-release.ps1:21` | Every update fails ("the update holds a file foreign to the package"). Even if allowed, the MCP sidecar would never be replaced. |
| F3 | The release asset and its root folder must be named `Aurora-Viewer-{tag}-windows-x64`. | `feed.rs:220`, `package.rs:163` | Our releases are not seen as updates (no archive found), or they ship under Aurora's name, which `TRADEMARKS.md` does not allow. |
| F4 | `update-policy.json` (upstream file) sets `minimum_supported_version: "0.9.1"`. | `/update-policy.json` | With our own version numbers (0.x below 0.9.1), the policy script throws, and the viewer refuses the policy as "invalid update floor" (`feed.rs:236-241`). |
| F5 | `AURORA_VERSION_TAG` must be the tag of the checked-out commit itself. | `aurora-version/build_support.rs:36-46` | The workflow must create the tag (at least locally) **before** building. |
| F6 | Nothing stops a viewer started by the session host (`--managed`) from offering "restart to update". The session-host plan says it must not (`folderstorm/plans/session-host.md:173`); this was not built. | `crates/aurora-viewer/src/updater.rs:164-168` (no `managed` check found) | A managed character could restart itself out of the host, and the updater expects the other instances to be closed (`install.rs:560`). Not checked at run time. |
| F7 | No Folderstorm2 release has ever been published (the releases API returns an empty list; the only tag is `v0.0.1-beta.20261110.1`). | GitHub | Good news: there are no installed copies yet, so all the renames can land **before** the first release with no migration code. |

---

## 1. Program name at packaging

The decision: shipped builds contain `Folderstorm2.exe`; cargo still builds
`target/<profile>/aurora-viewer.exe`. Packaging copies it under the new name.

### What expects the file name at run time

| Place | What it does with the name | Needs for `Folderstorm2.exe` |
|---|---|---|
| `crates/aurora-viewer/src/url_scheme.rs:218-233` (`win::repair`) | At start (release builds, outside demo: `:714-724`), a `secondlife://` registration whose program is **named** `aurora-viewer.exe` and no longer exists is pointed at this viewer. Registration itself (`:161-172`) and "is it ours" (`:185-191`) use `current_exe()`, not the name. | **Hook**: compare with `folderstorm_brand::EXE`. Bonus: today a Folderstorm2 build can take over a stale registration of a real Aurora Viewer; with the brand name it no longer can. |
| `crates/aurora-viewer/src/overlay_hooks.rs:51` | Log advice: "add aurora-viewer.exe to RivaTuner". Log text only (`:108-113`). | Optional hook (wrong name in a log line). |
| `crates/aurora-render/src/adapter_choice.rs:179` | Log line naming `aurora-viewer.exe` for the NVIDIA Control Panel. | Optional hook. The **visible** text is `ui-options-graphics-card-no-discrete` (en/fr/ja `.ftl`, e.g. `en.ftl:8726`): fix it in our own `crates/aurora-i18n/src/folderstorm.rs` rewrite, no upstream change. Same for the usage line of `cli-help` (`en.ftl:10918`). |
| `crates/aurora-viewer/src/system_notify.rs:183` (`DESKTOP_ENTRY`) | Linux freedesktop notifications only. Windows uses the AppUserModelID, already from the brand (`:319`, `WINDOWS_AUMID`). | Nothing for Windows. |
| `crates/aurora-viewer/src/cli.rs:247` (`DESKTOP_ID`) | Linux Wayland app_id / X11 class (`#[cfg_attr(not(target_os = "linux"), ...)]`). | Nothing for Windows. Linux packaging is a separate question. |
| `crates/aurora-viewer/src/uninstall.rs:343-353` | Linux menu entries named `aurora-viewer.desktop`. The viewer itself is found with `current_exe()` (`:351-353`). | Nothing for Windows. |
| `crates/aurora-render/src/adapter_choice.rs` (driver hints) | `NvOptimusEnablement` exports, linked with `rustc-link-arg-bin=aurora-viewer` (`crates/aurora-viewer/build.rs:32-34`). | Nothing: a rename after linking keeps the exports. |
| `aurora-updater` `install.rs` / `package.rs` | See F1, F2, F3. | **Hooks** (below). |
| Crash handler (`crates/aurora-viewer/src/crash/`) | The watcher and the crash window start with `Command::new(current_exe())` (`crash/mod.rs:156-157`). Stack module names come from the PDB file name recorded in the exe (`crash/native.rs:580-596`), so they stay `aurora_viewer`; the symbol files (`aurora_viewer.pdb/<id>/aurora_viewer.sym`) and the triage script's `$OwnModule = 'aurora_viewer'` (`scripts/tools/crash-triage.ps1:14`) keep working. | Nothing. Run `dump_syms` on `target/release/aurora-viewer.exe` (the renamed copy works too). |
| Windows resources, `crates/aurora-viewer/build.rs:91-99` | `OriginalFilename` = `aurora-viewer.exe`; `ProductName` / `FileDescription` are the literal `"Folderstorm2"`. | **Hook**: add `folderstorm-brand` as a build-dependency and read `PRODUCT` and `EXE` (also removes the product literal from an upstream file, see BRANDING.md). Cosmetic (Properties › Details), but code-signing services compare it. |
| `folderstorm_characters/host/process.rs:195` | The session host starts each character with `Command::new(std::env::current_exe()?)`. | **Fine.** It starts whatever exe the host runs from, whatever its name, so host and characters are always the same build. |
| `crates/aurora-viewer/src/mcp_bridge/setup.rs:28-31` (ours) | The MCP sidecar is looked up beside `current_exe()` as `folderstorm-mcp.exe`. | Fine. Could read a brand constant. Needs F2 fixed. |

### Proposed brand constants

In `crates/folderstorm-brand/src/lib.rs` (our crate, no conflict):

```rust
/// File name of the viewer in shipped builds: packaging renames
/// `target/release/aurora-viewer.exe` to it.
pub const EXE: &str = "Folderstorm2.exe";
/// The MCP sidecar shipped beside it (FEATURES.md 4).
pub const MCP_EXE: &str = "folderstorm-mcp.exe";
/// Release asset names: `<prefix>-<tag>-windows-x64.zip`, its root folder,
/// `.sha256` and `-symbols.zip`.
pub const RELEASE_PREFIX: &str = "Folderstorm2";
pub const RELEASES_DOWNLOAD: &str = "https://github.com/Sodie-Pop/Folderstorm2/releases/download";
```

Plus a test that none of them equals Aurora's (`aurora-viewer.exe`,
`Aurora-Viewer`), next to `identity_never_collides_with_aurora`.

### Hooks (smallest set)

| Upstream file | Change |
|---|---|
| `aurora-updater/src/install.rs:14` | `const EXE: &str = folderstorm_brand::EXE;` |
| `aurora-updater/src/install.rs:58-66` | add `folderstorm_brand::MCP_EXE` to `PACKAGE` |
| `aurora-updater/src/package.rs:163`, `:170` | root folder and exe from the brand |
| `aurora-updater/src/feed.rs:15`, `:220` | `DOWNLOADS` and the expected asset name from the brand (`:15` is a fork literal today) |
| `crates/aurora-viewer/src/url_scheme.rs:226` | `folderstorm_brand::EXE` |
| `crates/aurora-viewer/build.rs:91-94` | names from the brand (build-dependency) |
| optional: `overlay_hooks.rs:51`, `adapter_choice.rs:179` | `concat!` with a new `folderstorm_brand::exe!()` macro, like `product!()` |

**Merge cost.** Upstream's updater tests and manual test script write the
names as literals: `crates/aurora-updater/tests/updates.rs` (about 19 lines,
for example `:127`, `:181`) and `scripts/test-update.ps1:32-46`. Changing
`EXE` and the prefix breaks them, so they need edits too, and each upstream
change to those tests can then conflict. Two ways:

- **Recommended:** first offer upstream a no-behaviour-change PR, "name the
  Windows package files in one place" (`install::EXE`,
  `package::archive_name(tag)`, tests using them). Upstream gains tidier
  code; we are then left with one-line value hooks.
- If upstream says no: edit the test literals to use the constants, marked
  `// Folderstorm:`, and accept small conflicts at syncs.

Local cargo builds stay `aurora-viewer.exe`. They never update anyway: only
release builds whose version is a published tag check for updates
(`crates/aurora-viewer/src/updater.rs:164-174`).

---

## 2. Updates from GitHub releases

`aurora-updater` already reads only this fork's releases
(`feed.rs:11-14`, `folderstorm_brand::RELEASES_API`). The viewer checks at
launch and then every 10 minutes by default (`updater.rs:4-9`), using ETags so
repeated checks are nearly free (`feed.rs:56-62`).

### What a release must contain

| Item | Rule | Code |
|---|---|---|
| Tag | `v` + SemVer: `v0.1.0`; beta `v0.2.0-beta.1` (first pre-release word exactly `beta`). Build metadata (`+…`) is never a release. | `feed.rs:39-50`, `:210`, `:267-269` |
| Pre-release flag | Must match the tag: set for `-beta…`, unset for plain versions. A mismatch is ignored. Other pre-releases (`-rc.1`) are ignored by both channels. Drafts are ignored (and hidden from anonymous API calls anyway). | `feed.rs:208-210` |
| Newer than installed | SemVer precedence; build metadata ignored. | `lib.rs:96-102` |
| Archive asset | `Folderstorm2-<tag>-windows-x64.zip` after the hooks (today `Aurora-Viewer-…`), size 1 byte to 2 GiB. | `feed.rs:17`, `:220-225` |
| Checksum asset | `<archive>.sha256`, at most 1 KB, exactly `<64 hex>  <archive name>` (two fields; a leading `*` on the name is accepted). Without archive **and** checksum the release is shown with only its page link. | `feed.rs:226`, `package.rs:26-27`, `:116-124` |
| Policy asset (optional) | `aurora-update-policy.json`: `schema: 1`, `version` = tag without `v`, optional `minimum_supported_version`, `cache_invalidation_before`, `settings_reset_before` (stable versions ≤ the release), `grace_hours` 0–72. Max 16 KB. | `feed.rs:16`, `:231-249`, `:293-304`; `docs/UPDATES.md:93-139` |
| Archive layout | One root folder named exactly like the zip without `.zip`; every entry inside it; no links, no `\`, no Windows-reserved names; at most 50,000 entries and 4 GiB unpacked. | `package.rs:135-160`, `:162-239` (root check `:190-197`) |
| Package content | Top level only from `PACKAGE`: the exe, `assets/`, `LICENSE`, `NOTICE.md`, `README.md`, `version.txt`, `aurora-install.json` (+ `folderstorm-mcp.exe` after F2). Exe, `assets/` and `LICENSE` required. | `install.rs:58-66`, `:160-175` |
| Manifest | `aurora-install.json` = `{"schema": 1, "version": "<tag without v>"}`. `package-release.ps1:41-46` writes it from `--version`. | `package.rs:164-168` |
| Download hosts | Only `github.com`, `api.github.com`, `release-assets.githubusercontent.com`, `objects.githubusercontent.com`, over HTTPS, redirects included. | `feed.rs:91-108`, `:131-150` |

`Compress-Archive` must run in PowerShell 7 (`shell: pwsh`), as today: the
zip must use `/` separators (`package.rs:137`).

**Channels.** Stable (default) and Beta (`feed.rs:25-45`, Preferences ›
Language and updates). Folderstorm (C++) said "Nightly"; here it is Beta, and
we have no nightly job (upstream's `nightly.yml` is disabled, `CI.md`). A beta
is simply a release made by hand with a `-beta.N` version.

**Version in the build.** `crates/aurora-version/build.rs:23-45`: with
`AURORA_VERSION_TAG=v0.1.0` the version is `0.1.0`, but only if
`git describe --tags --match v[0-9]*.[0-9]*.[0-9]*` says the commit **is**
that tag (`build_support.rs:36-46`, F5). Without it the version is
`<last tag>+<distance>.g<hash>`, which never updates. Only fork tags exist on
`origin` (upstream's tags are not fetched or pushed): **never push upstream
tags to this repository**, or dev versions would read `0.9.x`.

**Version numbers.** Upstream is at `v0.9.12-beta…`. We should use our own
SemVer line, starting at `v0.1.0` (above the stray `v0.0.1-beta.20261110.1`
tag). Because of F4, our workflow must not use upstream's
`update-policy.json`: keep our own `folderstorm/update-policy.json` (or no
policy asset at all at first, which makes every update optional).

### Workflow change

`.github/workflows/folderstorm-release.yml` (ours) today builds an artifact
only. Proposed shape, mirroring upstream's split between a read-only build
and a write-only publish (`release.yml:43-45`, `:131-158`):

```yaml
on:
  workflow_dispatch:
    inputs:
      version:   # "0.1.0" or "0.2.0-beta.1"
        required: true
        type: string
permissions:
  contents: read
jobs:
  windows:            # read-only token
    steps:
      - checkout (fetch-depth: 0, persist-credentials: false)
      - check: running on main; version matches
        ^[0-9]+\.[0-9]+\.[0-9]+(-beta\.[0-9A-Za-z.]+)?$ ; tag v$V not on origin yet
      - git tag -a "v$V" -m "Folderstorm2 v$V"   # local only: aurora-version needs it (F5)
      - build: AURORA_VERSION_TAG=v$V cargo build --release --locked
               -p aurora-viewer -p folderstorm-mcp --features aurora-viewer/xr-static-loader
      - verify: `aurora-viewer.exe --version` prints "<PRODUCT> $V"
               (upstream's verify-version.ps1 expects "Aurora Viewer", so ours)
      - symbols: dump_syms (pinned, as release.yml:75-89) -> <prefix>-v$V-windows-x64-symbols.zip
      - (later) sign the two exes
      - package-release.ps1 -> folder <prefix>-v$V-windows-x64 with Folderstorm2.exe
      - Compress-Archive; write "<hash>  <name>" to <zip>.sha256
      - (optional) aurora-update-policy.json from folderstorm/update-policy.json
      - (later) Inno Setup installer + its .sha256
      - upload-artifact
  publish:            # never on pull_request
    needs: windows
    permissions:
      contents: write
    steps:
      - download-artifact
      - gh release create "v$V" <files> --draft --target "$GITHUB_SHA"
          --title "Folderstorm2 v$V" --notes-file folderstorm/release-notes-header.md
          --generate-notes [--prerelease if $V contains "-"]
```

The human reads the draft and clicks **Publish**. GitHub creates the tag at
publish time, at `--target`. Until then nothing is visible to viewers
(drafts are skipped, `feed.rs:210`). Notes:

- **`permissions: contents: write`** is needed to create the release; give it
  to the publish job only.
- The product name, slug and prefix are read from `folderstorm-brand` with
  `sed`, as the "Names" step already does (`folderstorm-release.yml:49-58`).
- **Release notes header.** Upstream's `.github/release-notes-header.md`
  carries the Linden Lab disclaimer, support and privacy links "(Third-Party
  Viewer Policy §1.c)" (`release.yml:127-130`), all pointing at Aurora. We
  need our own `folderstorm/release-notes-header.md` with our support link
  and **our privacy policy URL** (human).
- Keep the existing manual-artifact mode (no version input = no release) for
  test builds, or keep it as a separate job.
- `.github/README.md:37-39` ("no official releases yet") changes with the
  first release.

**Reusable from upstream.** `release.yml`: the build flags, `dump_syms`
symbols step, checksum format, publish split and `--prerelease` rule.
`scripts/package.ps1`: already mirrored by `package-release.ps1` (upstream's
expects "Aurora Viewer <version>", `package.ps1:50`). `scripts/write-update-policy.ps1`
and `scripts/tools/update-policy.ps1`: not reusable as is (they read the root
`update-policy.json` and Aurora's tag annotations); copy the validation rules.
Upstream's `release.yml` stays disabled (`CI.md`); a tag created by our
workflow with `GITHUB_TOKEN` would not start it anyway.

### Security

**Today: SHA-256 only, no signature.** The checksum comes from the same
release as the archive (`package.rs:28-32`); the download must match its
size and hash (`package.rs:98-100`). No signature check exists anywhere in
the crate. What this protects against, and what not:

| Threat | Covered? |
|---|---|
| Corrupt or truncated download | Yes (size + SHA-256) |
| Network attacker, fake server | Yes (HTTPS, host allow-list, `feed.rs:91-108`) |
| Someone who can upload release assets (stolen GitHub login or token, a malicious workflow change) | **No**: they replace the zip and its `.sha256` together |

Options for a real signature:

1. **minisign / Ed25519 detached signature.** Publish `<zip>.minisig`; the
   public key is a `folderstorm-brand` constant; the updater checks it before
   extracting (one hook in `package.rs::download` calling a new file of ours,
   with the small pure-Rust `minisign-verify` crate). Worth offering upstream
   too.
   - *Key as a GitHub secret* (in an environment that needs the human's
     approval): automatic, but whoever controls the maintainer's GitHub
     account can still run the workflow and sign. Limited gain for a
     one-person project.
   - *Key offline on the human's PC*: the human downloads the draft's zip,
     signs it, uploads the `.minisig`, then publishes. Real protection
     against a GitHub-side compromise, at the cost of a manual step every
     release. Agents never see the key.
   - Downsides either way: a lost key means old installations refuse every
     new update until reinstalled by hand (plan a second, backup public key);
     a verification bug blocks all updates; one more upstream hook.
2. **Check the Authenticode signature** of the new exe, once builds are
   code-signed. Weak with SignPath's free certificate: its publisher is
   "SignPath Foundation", shared by many projects.

Recommendation: ship the first releases with SHA-256 (as upstream does),
2FA on the GitHub account, branch protection on `main` and the publish job
behind a GitHub environment with required approval. Add minisign with an
**offline** key later if the human accepts the manual step (question 6).

### Code signing (SmartScreen)

Without signing, Windows shows "Windows protected your PC" for the
downloaded zip's exe or installer (More info › Run anyway), and some
antivirus tools are quicker to flag unknown binaries. **Updates installed by
the viewer itself are not affected**: they are downloaded by the viewer, not
a browser, so they carry no "downloaded from the internet" mark.

Upstream planned SignPath but has not set it up (`docs/UPDATES.md:77-80`,
`TASKS.md:1395`). Options, all to be checked against current terms:

| Option | Cost | Shown publisher | Notes |
|---|---|---|---|
| **SignPath Foundation** (free for open source) | Free | "SignPath Foundation" | Requires an OSI licence (GPL-3.0: yes) and a public repository, fully automated CI builds from that source, no proprietary components, MFA for the team, a "Free code signing provided by SignPath.io, certificate by SignPath Foundation" statement, and a manual approval of each signing request. To check: whether compiling in `FOLDERSTORM_IMAGE_HOST_TOKEN` (`CI.md`) fits "built from the repository's source". Sources: [signpath.org/terms](https://signpath.org/terms), [SignPath for open source](https://about.signpath.io/product/open-source). |
| **Azure Artifact Signing** (formerly Trusted Signing) | Monthly fee per tier; billing starts when the account is created. Check the [pricing page](https://azure.microsoft.com/pricing/details/artifact-signing/). | The human's verified name (a real name, shown to every user) | Identity validation. During preview, individual developers were reportedly limited to the US and Canada ([Microsoft Q&A, 2026-03](https://learn.microsoft.com/en-us/answers/questions/5810735/cant-create-a-new-trusted-signing-individual-ident)); check current eligibility. |
| Classic OV / EV certificate from a CA | Usually a few hundred USD a year, plus a hardware token or cloud HSM (required for code signing keys since 2023) | The human's or a company's name | Reputation with SmartScreen still builds up over downloads; check whether EV still gives any head start. |

Recommendation: first releases unsigned (betas stay unsigned, as upstream
decided: `docs/UPDATES.md:77-80`); apply to SignPath Foundation once the
release workflow is stable. Its approval step fits the draft-release flow.

---

## 3. Installer

Today the human starts the session host from a hand-made `.cmd`. The flags
are `--session-host` and `--profile <name>` (`crates/aurora-viewer/src/cli.rs:36-42`,
parsed at `:131-136`). Upstream has **no installer**: `url_scheme.rs:4-6` and
`uninstall.rs:10` say so, `packaging/` only holds Linux, macOS and licence
files, and `docs/UPDATES.md:148-150` says Velopack was studied and not kept.
Folderstorm (C++) shipped NSIS and Velopack installers (FEATURES.md, "Not
porting").

### Options

| Tool | For | Against |
|---|---|---|
| **Inno Setup** | Free, mature, simple script; per-user install (`PrivilegesRequired=lowest`); Start menu, optional desktop shortcut and registry tasks built in; Pascal script for custom uninstall logic; command-line compiler for CI. | Another script language to maintain (small). Check that GitHub's Windows image still includes it, or install it in the job. |
| WiX / MSI (or `cargo-wix`) | Standard for companies, Group Policy deployment. | MSI owns its files: our self-updater replacing them fights MSI repair and upgrade rules. Per-user MSI is awkward. Most effort. |
| NSIS | What the Legacy Viewer and Folderstorm (C++) used (`installer_template.nsi`). | Script language is more error-prone; no real gain over Inno here. |
| `cargo-packager` | Generates NSIS or WiX from Cargo metadata. | Less control over the parts that matter here (updater layout, link handler, uninstall). |
| Velopack | Installer + delta updates. | Replaces `aurora-updater`; upstream rejected it (`docs/UPDATES.md:148-150`). |

### What the installer needs

- **Per-user only, no admin.** Default folder `%LOCALAPPDATA%\Programs\Folderstorm2`.
  This is required by the updater: it writes its work folder
  `.aurora-update-<uuid>` and its lock `.<folder>.update.lock` **beside** the
  installation, in the parent folder (`install.rs:99-104`, `:240-247`), and
  refuses a folder at a drive root (`install.rs:128-140`). Under `C:\Program
  Files` the parent is not writable without admin, so every update would
  fail ("cannot write next to installation"). Do not offer per-machine.
- **Same files as the zip.** The installer lays down the exact package
  (`Folderstorm2.exe`, `folderstorm-mcp.exe`, `assets\`, notices,
  `aurora-install.json`), so the updater sees a full package (`install.rs:144-148`)
  and updates it in place. Inno's own `unins000.exe` / `.dat` sit beside it;
  the updater leaves any other file alone (`install.rs:69-70`).
- **Start menu:** "Folderstorm2" (no arguments) and "Folderstorm2 Session
  Host" (`--session-host`). The updater keeps the arguments when it restarts
  (`install.rs:261-264`), so a host restarted after an update stays a host.
  Do **not** set an AppUserModelID on the shortcuts: the process does not set
  one itself, so the pinned shortcut and the running window would show as two
  taskbar buttons. Notifications already work through the registered
  `Folderstorm2.Viewer` id (`system_notify.rs:8-11`, `:372-377`).
- **Desktop shortcut:** an unchecked task "Create a desktop shortcut".
- **`secondlife://` handler:** an optional task "Open secondlife:// links with
  Folderstorm2", **unchecked** by default (it would take links away from
  another viewer). Write exactly what the viewer writes, under
  `HKCU\Software\Classes\{secondlife,hop,x-grid-location-info}` with the
  command `"<exe>" -url "%1"` (`url_scheme.rs:161-172`), so the viewer
  recognises it as its own (`:185-191`) and Preferences › Interface can
  remove it. Users can also turn it on later from Preferences.
- **Uninstaller** that respects `crates/aurora-viewer/src/uninstall.rs`.
  `--uninstall` (`cli.rs:117`, `uninstall.rs:436-465`) removes, with no
  questions: the link classes only when they open this exe, the notification
  id, **saved passwords, second-factor tokens, the cache key and the GitHub
  authorization**, and the Blender add-on (`uninstall.rs:13-20`). It deletes
  no folder and prints the folders left. So:
  - the uninstaller asks "Also remove your settings, saved passwords and
    cache?", default **No**;
  - No: Pascal code removes only the link classes whose command points at
    `{app}\Folderstorm2.exe`, and the `AppUserModelId\Folderstorm2.Viewer` key;
  - Yes: run `"{app}\Folderstorm2.exe" --uninstall` first, then delete the
    default folders of `project_dirs()` (`org` / `Folderstorm` /
    `Folderstorm2`, `crates/folderstorm-brand/src/lib.rs:35-46`) and the
    profile caches. Never delete a cache location or chat transcript folder
    the user chose: those stay listed, as the viewer does.
  - Remove the whole `{app}\assets` tree (an update may have added files Inno
    did not install).
  - Leave any `.aurora-update-*` folder beside the installation: one that
    still holds `backup\` **is** the previous version after a failed update
    (`install.rs:359-364`, `docs/UPDATES.md` "Replacement and rollback").
- **Running viewer:** use Inno's `CloseApplications` (Restart Manager) so an
  upgrade or uninstall asks to close the viewers first.
- **After a self-update**, "Apps & features" still shows the installed
  version (`DisplayVersion` of Inno's uninstall key). Cosmetic; an optional
  hook could refresh it after a successful update.
- **Build in our workflow:** `folderstorm/installer/folderstorm2.iss`, compiled
  with `ISCC.exe /DAppVersion=$V /DSource=<package folder>`, names read from
  the brand by `sed`, output `Folderstorm2-v$V-windows-x64-setup.exe` and its
  `.sha256`, attached to the same draft release. The updater ignores it (it
  looks for the exact zip name). Signed in the same step as the exes, once
  signing exists.

**Where the updater writes, installed copy:** `%LOCALAPPDATA%\Programs\.aurora-update-<uuid>\`
(download, extraction, backup, helper copy) and
`%LOCALAPPDATA%\Programs\.Folderstorm2.update.lock`; cache and settings
backups `*.aurora-before-update-<uuid>` beside them when a policy asks
(`docs/UPDATES.md`). One side note: the sweep of finished
`.aurora-update-*` folders (`install.rs:367-385`) also tidies those of an
Aurora Viewer installed in the same parent folder. Harmless (finished ones
only), so no hook.

**MCP.** An installer gives the sidecar a stable path
(`%LOCALAPPDATA%\Programs\Folderstorm2\folderstorm-mcp.exe`), which matters
because *Copy configuration* writes that path into the assistant's settings
(`mcp_bridge/setup.rs:28-31`). Another reason for the installer.

### Alternative: "Create shortcuts" inside the viewer

For zip users, a first-run choice (or a Help menu item) could create the two
Start menu shortcuts and a desktop one with `IShellLinkW` (no such code
exists today). Our own module plus one hook, no installer. But it can't
register an uninstaller, and the zip folder can move, breaking the shortcuts
and the MCP path.

### Recommendation

**Inno Setup, per-user only, plus the zip.** The installer does the first
install and the shortcuts; the in-viewer updater keeps updating the
installed folder from the zip, as it already does. This adds no change to
the updater's logic and no upstream file. Leave the in-viewer "Create
shortcuts" for later, only if zip users ask.

---

## 4. Crash reports

### What Aurora already does

Read in `crates/aurora-viewer/src/crash/` and `docs/BUG_REPORTING.md`:

- **Local, per session.** Each session gets a folder under
  `<logs>/crashes/sessions/<id>/` (`crash/mod.rs:76-86`): `session.json`,
  `session.log`, the native trace, `public-report.md`, private extras such as
  `gpu-errors-private.txt`. A watcher process (`--crash-watch`,
  `crash/mod.rs:121-130`, `:243`) sees the end of the session and shows the
  crash window at the next start.
- **No minidump.** Windows: a vectored / unhandled-exception handler writes
  `module+offset` stacks with each module's PDB identity (`crash/native.rs`).
  Linux: same text from a signal handler, "no minidump, nothing sent by itself"
  (`crash/linux.rs:18-21`).
- **Scrubbed public report.** `public-report.md` uses a closed vocabulary: no
  raw addresses, paths, panic messages or log text (`crash/privacy.rs:1-8`;
  `docs/BUG_REPORTING.md:44-52`). The window has "Open report folder" and
  "Copy trace" (`crash/window.rs:251`, `:257`).
- **Sending.** Only as a **public GitHub issue in the tester's name**, after
  they read it and write what they were doing (`crash/mod.rs:1-6`;
  `docs/BUG_REPORTING.md:11-52`). Preferences › Privacy › Crash reports: Ask
  (default), Always, Never. Direct posting needs a GitHub App: the fork has
  none (`crash/github.rs:19-21`, empty `CLIENT_ID`), so in Folderstorm2 the
  path is "Open in the browser", a pre-filled issue on this repository
  (`crash/github.rs:520-530`). **Nothing is sent anywhere else.**
- **Symbols and triage.** Upstream's release publishes Breakpad symbols made
  by `dump_syms` (`release.yml:71-89`); its Crash triage workflow symbolises
  crash issues (`docs/BUG_REPORTING.md:172-248`, fixtures in
  `scripts/test-crash-triage.ps1`). The script uses `GITHUB_REPOSITORY` and
  the glob `*-windows-x64-symbols.zip` (`scripts/crash-triage.ps1:44`,
  `:67-68`), so it would work on the fork once we publish symbols. It is
  disabled on the fork (`CI.md`).

So the fork already has local crash reports and a privacy-safe report. What
is missing: symbols in our releases, and a way to receive reports without
asking each tester to use GitHub.

### Options

| Option | What it takes | Merge cost |
|---|---|---|
| **A. Local only**, polished | Symbols in releases (section 2); re-enable *Crash triage* in the Actions tab (no file change); maybe a "Copy report" next to "Copy trace". Testers attach the report to a bug report. | None to tiny. |
| **B. Sentry, stack traces only** | Turn the existing `public-report.md` data into a Sentry event (stack frames + module identities; Sentry can symbolise from uploaded Breakpad `.sym` files) and send it, opt-in, from our own module with one hook in the crash window. No minidump, no memory. Needs checking: Sentry's handling of module+offset frames. | Small: one hook. |
| **C. Sentry with minidumps** (`sentry-native` / Crashpad, as Folderstorm (C++) did) | A second crash handler in-process, an out-of-process handler exe to ship, PDB upload. | **High**: it competes with Aurora's own exception handlers in `crash/native.rs`, which upstream changes often. |
| **D. Own endpoint** on the maintainer's server | A small HTTPS endpoint that accepts the public report (like B's payload); or a lighter self-hosted Sentry-compatible server (GlitchTip, Bugsink). Full Sentry self-hosting needs a large server. | Small in the viewer; the server is the human's to run and secure. |

### Downsides, in plain words

- **Privacy.** A minidump is a snapshot of the program's memory. In a viewer
  that memory can hold chat and IM text, avatar names, inventory, and the
  login session's secrets. Scrubbing tools cannot reliably clean raw memory.
  Aurora's text report avoids this on purpose: option B keeps that; option C
  loses it.
- **Consent.** Sending anything automatically needs an opt-in, off by
  default, that the user can turn off, and a **privacy policy** saying what is
  sent, where, and how long it is kept. Folderstorm (C++) did the same
  (default Off, one-time invitation). Today Folderstorm2 links Aurora's privacy
  policy (`crates/aurora-viewer/src/links.rs:19-20`), which does not describe a
  fork sending crashes to Sentry.
- **Second Life's Third Party Viewer Policy.** The repository cites it for the
  release notes' disclaimer, support and privacy links (`release.yml:127-130`,
  "§1.c"), the viewer channel (`folderstorm-brand/src/lib.rs:71-74`), the
  login device ids (`machine_id.rs:1-4`) and viewer statistics
  (`aurora-net/src/viewer_stats.rs:1-4`). Its clauses on collecting and
  sending user data were not checked here: `secondlife.com` is not reachable
  from this session. **Needs checking by the human** before any automatic
  sending.
- **Size and work.** Symbols must be uploaded for every release, from CI. That
  needs a Sentry auth token as a GitHub secret, set by the human (agents never
  handle it). With option C, also the PDBs (large) and a handler exe.
- **The DSN is semi-public.** It is compiled into the viewer, so anyone can
  read it and send fake or junk events. Sentry has rate limits and filters,
  but spam can use up the quota.
- **Free quota.** Sentry's free Developer plan is reported as 5,000 errors a
  month for one user (check [sentry.io/pricing](https://sentry.io/pricing/));
  a crash loop or spam can use it up.
- **Merge cost** with upstream's crash code: low for A, B, D; high for C.

### Recommendation (phased)

1. **Phase 1, now:** option A. Symbols in every release; re-enable Crash
   triage on the fork; reports stay local, and testers attach them.
2. **Phase 2, if the human wants automatic reports:** option B (or D with
   the same payload), opt-in, off by default, stack traces and fixed fields
   only, never minidumps, never logs. Only after a privacy policy exists and
   the TPV policy is checked.
3. **Not recommended:** option C (minidumps), for privacy and merge cost.

**The human provides (phase 2):** a Sentry project and its DSN (as a GitHub
secret or variable), a Sentry auth token for symbol upload (GitHub secret),
and a privacy-policy URL. Agents never type, store or read these.

---

## 5. Bug-report links

Where they live today (all point at this repository's GitHub issues):

| Place | What |
|---|---|
| `crates/folderstorm-brand/src/lib.rs:111-114` | `REPOSITORY`, `GITHUB`, `NEW_ISSUE` |
| `crates/aurora-viewer/src/links.rs:8-11` | `NEW_ISSUE`, opened by `App::open_issue_chooser` (`app.rs:16040-16046`) |
| Help menu | `ui/bars.rs:1647-1648` ("Report a bug") |
| Top bar and island bar | `ui/bars.rs:2393-2396`, `ui/bars/islands.rs:356-363` |
| Login screen | `ui/login/chrome.rs:80-85` |
| Crash window | `crash/github.rs:13-15` (fork literals `API`, `ISSUES`), "Open in the browser" `crash/github.rs:520-530` / `crash/window.rs:489`, issue link after publishing `crash/window.rs:334` |
| Built-in form (disabled) | `bug_report/github.rs:9-16`, `ui/bug_report.rs:431` |
| Session host settings | `folderstorm_characters/host/ui/settings.rs:867-881` (says to use the viewer's Help menu) |
| Docs | `folderstorm/BRANDING.md:67-69`, `folderstorm/FOLLOW-UPS.md` (2026-11-12 row); upstream's `docs/BUG_REPORTING.md:1-9`, `README.md:1005-1010`, `.github/ISSUE_TEMPLATE/config.yml` (feature request and security links go to **Aurora**), `.github/release-notes-header.md` (Aurora's support links) |

Plan, once the human gives the address: a `folderstorm_brand::BUG_REPORTS`
constant; `links.rs:11` reads it (one-line hook, already marked); the session
host text follows. Decide separately whether the crash window keeps its
GitHub-issue route or opens the server too (it builds a pre-filled GitHub
URL, so the server would need an equivalent form). Tracked in
[FOLLOW-UPS.md](../FOLLOW-UPS.md). Folderstorm (C++) sent ordinary bug
reports to a public tracker on the maintainer's site, with no build data in
the URL.

---

## 6. Phases and PRs

Small PRs, in order. 🔑 = needs the human's input, account or secret.

| # | PR | Contents | Human |
|---|---|---|---|
| 0 | (upstream, optional) "Name the Windows package files in one place" | `install::EXE`, `package::archive_name`, tests use them. No behaviour change. | Agrees to propose it upstream |
| 1 | `feat(release): ship Folderstorm2.exe and update it` | Brand constants; updater hooks (F1–F3); `url_scheme.rs:226`; `build.rs` names; i18n rewrite of the exe name; `package-release.ps1` renames the exe and names the folder `Folderstorm2-v<V>-windows-x64`; tests. | Test an update between two local packages (fork version of `scripts/test-update.ps1`) |
| 2 | `ci(release): draft GitHub release from a version` | Workflow of section 2: version input, local tag, `AURORA_VERSION_TAG`, verify, symbols, zip, `.sha256`, draft release, `contents: write` on publish only; `folderstorm/update-policy.json` (F4); `folderstorm/release-notes-header.md`; CI.md and README updates. | 🔑 privacy-policy URL and support link for the header; runs the workflow; publishes a beta pair to test a real update |
| 3 | `fix(characters): updates with the session host` | Managed viewers do not offer restart-to-update (F6); the host says "close all characters to update", or does "quit all and update". | Tests with several characters |
| 4 | `feat(installer): per-user Inno Setup installer` | `folderstorm/installer/folderstorm2.iss`, workflow step, shortcuts, link task, uninstaller of section 3. | Installs, updates, uninstalls on Windows |
| 5 | `docs(crash): crash reports on the fork` + Actions setting | Re-enable Crash triage in the Actions tab; document it in CI.md. Symbols already come from PR 2. | Clicks "Enable workflow" |
| 6 | `ci(release): code signing` | SignPath step for the exes and the installer, stable releases only. | 🔑 applies to SignPath Foundation, sets its secrets, approves each signing |
| 7 | `feat(links): bug reports to the maintainer's server` | `BUG_REPORTS` constant, hooks, session host text, docs (section 5). | 🔑 the address |
| 8 | `feat(crash): opt-in crash reports (stack traces only)` | Option B or D, consent setting, symbol upload from CI. | 🔑 DSN or endpoint, auth token secret, privacy policy, TPV check |
| 9 | (optional) `feat(updater): signed updates` | minisign check, public key in the brand. | 🔑 creates and keeps the key; signs each release |
| 10 | (optional) `feat(shortcuts): create shortcuts from the viewer` | For zip users. | Decides if wanted |

PRs 1–2 are enough for a first public release (zip, unsigned). PR 3 should
land before telling session-host users to update in place. FEATURES.md 0.3
is done after PR 2 (and 3); 0.4 after PR 5 (phase 1) or PR 8 (phase 2).

---

## 7. Questions for the human

1. **Version numbers:** our own SemVer line, first release `v0.1.0`, betas
   `v0.2.0-beta.1`? *Recommended: yes.*
2. **Update policy:** start with no policy file in releases (every update
   optional), and add `folderstorm/update-policy.json` when one is needed?
   *Recommended: yes.*
3. **Installer:** Inno Setup, per-user only, with the zip still published?
   *Recommended: yes.*
4. **Link handler in the installer:** offer it unchecked? *Recommended:
   unchecked; Preferences can turn it on.*
5. **Uninstaller and personal data:** ask, default "keep settings and saved
   passwords"? *Recommended: yes.*
6. **Update signature:** SHA-256 only for now, minisign with an offline key
   later? *Recommended: yes, if you accept signing each release by hand.*
7. **Code signing:** first releases unsigned, then apply to SignPath
   Foundation (free; publisher shown as "SignPath Foundation")?
   *Recommended: yes. Azure shows your real name and costs a monthly fee.*
8. **Crash reports:** phase 1 (local + symbols + triage) now, and decide on
   automatic reports later, stack traces only and opt-in? *Recommended: yes;
   no minidumps.*
9. **Privacy policy and support contact:** where will the fork's privacy
   policy live? It is needed for the release notes (TPV policy) and before
   any automatic crash report. *Recommended: a page on your server, linked
   from `links.rs` and the release notes.*
10. **Bug reports:** when you give the server address, should the crash
    window's "Open in the browser" go there too, or stay on GitHub issues?
    *Recommended: the Help menu link moves first; crash reports stay on
    GitHub until phase 2 is decided.*

---

## 8. Decided

2026-11-19, by the human: the recommended answer to all ten questions of
section 7, with these notes.

- **Bug reports** go to **https://folderstorm.com/bugs/** (a
  `folderstorm-brand` constant). The Help menu, bars and login screen links
  move there with PR 7; the crash window stays on GitHub issues until phase
  2 of crash reports is decided (question 10).
- **The privacy policy and support contact** will be pages on the same
  server (question 9); their addresses are still to come and the release
  notes header waits for them.
- **Releases are built and uploaded by the human's own local agent**, on
  the Windows PC, following the human's build skill. So PR 2 also gives a
  local path, with the same result as the workflow:
  - `folderstorm/scripts/package-release.ps1` makes the exact release
    assets (archive named as the updater expects, its `.sha256`, the
    symbols) from a local `cargo build --release` with
    `AURORA_VERSION_TAG` set and the tag created first;
  - a new `folderstorm/RELEASING.md` gives the steps, written so that an
    agent can follow them: check the version, tag, build, package, verify
    `--version` and the checksum, `gh release create <tag> --draft` with
    the assets and notes, then the human reviews and publishes. Agents
    never handle the human's GitHub credentials: `gh` is already signed in
    on that PC by the human.
  - The `workflow_dispatch` workflow stays as the second way, sharing the
    same packaging script.
- **MCP configuration and spaces in paths.** Folderstorm (C++) had trouble
  configuring MCP clients when the folder of the `.exe` had a space in it
  (worked around with a link). The per-user installer puts the program
  under `%LOCALAPPDATA%\Programs\Folderstorm2`, and a Windows user name
  can hold spaces (`C:\Users\First Last\...`), so the configuration the
  viewer copies for MCP clients must quote and escape the path correctly in
  every format it offers (JSON with escaped backslashes, `claude mcp add`
  and other command lines with quotes, TOML for Codex). A separate small
  PR checks each format with a path holding spaces, with tests; see
  FEATURES.md 4.x.
