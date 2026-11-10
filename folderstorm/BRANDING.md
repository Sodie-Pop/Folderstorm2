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
- saved passwords, the second-factor entry, the disk cache key and the GitHub
  authorization in the system's credential store;
- the Windows notification identity, the `secondlife://` link handler's
  registration name, the Windows pipe and window names and the Linux bus name
  that hand a link to a running viewer;
- updates: the updater reads only this repository's releases, so it can never
  replace Folderstorm2 with Aurora Viewer.

A test in `folderstorm-brand` fails if one of these equals Aurora's.

## Interface text

`crates/aurora-i18n/src/folderstorm.rs`:

- "Aurora Viewer" (Japanese "Auroraビューア") becomes "Folderstorm2" in every
  message except the few of `KEEP` (the Aurora team badge, links to Aurora's
  website and wiki);
- a bare "Aurora" changes only in the messages listed in `BARE`, where it names
  the viewer. "Aurora" is also the name of the interface style (Aurora /
  Legacy), of grid-side items (`#Aurora` folders, the Aurora bridge, Aurora
  Library) and of the demo places: those keep it;
- French elision follows ("d'Aurora Viewer" → "de Folderstorm2").

At an upstream sync, look at new messages that say "Aurora" alone: add their
key to `BARE` when they name the viewer.

## Deliberately unchanged (for now)

- In-world names: `#Aurora` folders, `#Aurora Bridge`, Aurora AO, Aurora
  Library. Renaming them would orphan existing items.
- The executable `aurora-viewer(.exe)`, the crate names, the `AURORA_*`
  variables and the Linux desktop id: internal, and renaming them would touch
  many upstream files.
- Release packaging (`packaging/`, `scripts/package*.ps1`, `release.yml`):
  revisited with FEATURES.md 0.3, when Folderstorm2 publishes releases.
- The website, wiki, Discord, privacy policy and news links still point at
  Aurora's until the fork has its own (`crates/aurora-viewer/src/links.rs`).
- Crash and bug reports open a pre-filled issue on this repository in the
  browser. Sending them directly needs the fork's own GitHub App
  (FEATURES.md 0.4): put its public client id in `crash/github.rs` and
  `bug_report/github.rs`.
