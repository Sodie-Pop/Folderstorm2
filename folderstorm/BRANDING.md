# Folderstorm2 branding

How the viewer calls itself Folderstorm2 while staying close to upstream
Aurora Viewer. Aurora's `TRADEMARKS.md` does not allow a modified build to ship
as "Aurora Viewer" or with the wolf logo; the GPL requires keeping Aurora's
copyright and licence notices. Both hold here.

## Where the identity lives

| What | Where |
|---|---|
| Name, folders, credential-store entries, grid channel, system ids, repository links | `crates/folderstorm-brand/src/lib.rs` |
| The name in the interface text (en, fr, ja) | `crates/aurora-i18n/src/folderstorm.rs` rewrites the catalogs when they load |
| Logo, Windows icon, white silhouette (toolbar menu button) | `folderstorm/branding/` |
| Credit to Aurora Viewer | `NOTICE.md` (first paragraph and the logo row), shown in About › Licences; the About text "Copy to clipboard" says "based on Aurora Viewer" |

Aurora's files reach these through one-line hooks marked `// Folderstorm:`.

**Never write the product name or a fork identifier in Aurora's code
directly:** add it to `folderstorm-brand` and read it from there.

## What Folderstorm2 never shares with an installed Aurora Viewer

- settings, cache, data and log folders (`project_dirs()`: `org` /
  `Folderstorm` / `Folderstorm2`), the folder made inside a chosen cache
  location, the snapshots folder;
- saved passwords, the second-factor entry, the disk cache key, the GitHub
  authorization and the proxy's credentials in the system's credential store;
- the Windows notification identity, the `secondlife://` link handler's
  registration name, the Windows pipe and window names and the Linux bus name
  that hand a link to a running viewer;
- updates: the updater reads only this repository's releases, so it can never
  replace Folderstorm2 with Aurora Viewer. The two exceptions are the media
  plugins pack (CEF, LibVLC or GStreamer) and the Director's ffmpeg pack: the
  fork publishes neither, so they are downloaded from Aurora Viewer's releases
  (`MEDIA_PACK_DOWNLOADS`), and only the exact archive whose size and SHA-256
  the viewer pins is installed.

A test in `folderstorm-brand` fails if one of these equals Aurora's.

## Interface text

`crates/aurora-i18n/src/folderstorm.rs`:

- "Aurora Viewer" (Japanese "Auroraビューア") becomes "Folderstorm2" in every
  message except the few of `KEEP` (the Aurora team badge, a sample notecard
  that links to aurora-viewer.org);
- a bare "Aurora" changes only in the messages listed in `BARE`, where it names
  the viewer. "Aurora" is also the name of the interface style (Aurora /
  Legacy), of grid-side items (`#Aurora` folders, the Aurora bridge, Aurora
  Library) and of the demo places: those keep it;
- French elision follows ("d'Aurora Viewer" → "de Folderstorm2");
- the program's file name `aurora-viewer.exe` becomes `Folderstorm2.exe`,
  the shipped name (NVIDIA Control Panel hint, `--help` usage line).

## Shipped file names

Cargo still builds `target/<profile>/aurora-viewer.exe`; shipped Windows
builds hold it as **`Folderstorm2.exe`** (`EXE`), beside
`folderstorm-mcp.exe` (`MCP_EXE`), in release assets named
`Folderstorm2-<tag>-windows-x64…` (`RELEASE_PREFIX`).
`folderstorm/scripts/package-release.ps1` renames it. The updater looks for
these names in the installation and in the package (hooks in
`aurora-updater`'s `install.rs`, `package.rs` and `feed.rs`), so an update
also replaces the MCP sidecar. On Linux the executable keeps cargo's name.

At an upstream sync, look at new messages that say "Aurora" alone: add their
key to `BARE` when they name the viewer.

## Links on the maintainer's website

Through `crates/aurora-viewer/src/links.rs`, which reads `folderstorm-brand`:

| Link | Constant | Address |
|---|---|---|
| Website (About window, login screen) | `WEBSITE` | https://folderstorm.com |
| Help › "Folderstorm Help" (message `folderstorm-help-menu`, `SITE_NAME`) | `HELP` | https://folderstorm.com/help/ |
| Privacy policy (Preferences › Privacy › Your data; message `folderstorm-privacy-policy-hint`) | `PRIVACY` | https://folderstorm.com/privacy |
| "Report a bug" (login screen, top bar, Help menu) | `BUG_REPORTS` | https://folderstorm.com/bugs/ |

The crash window still files its reports as GitHub issues, below.

## Deliberately unchanged (for now)

- In-world names: `#Aurora` folders, `#Aurora Bridge`, Aurora AO, Aurora
  Library. Renaming them would orphan existing items.
- The cargo binary name `aurora-viewer(.exe)` (renamed only at packaging,
  above), the crate names, the `AURORA_*` variables and the Linux desktop id:
  internal, and renaming them would touch many upstream files.
- Upstream's release packaging (`packaging/`, `scripts/package*.ps1`,
  `release.yml`) stays Aurora's; the fork packages with
  `folderstorm/scripts/package-release.ps1`.
- The Discord, news feed and source links still point at Aurora's until the
  fork has its own (`crates/aurora-viewer/src/links.rs`).
- Crash reports open a pre-filled issue on this repository in the
  browser. Sending them directly needs the fork's own GitHub App
  (FEATURES.md 0.4): put its public client id in `crash/github.rs` and
  `bug_report/github.rs`.
