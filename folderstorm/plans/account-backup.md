# Account setup backup

Plan and record for [FEATURES.md](../FEATURES.md) row 0.6. **New**, asked by
the human; not in Folderstorm (C++). Branch `claude/account-backup`.

Goal: one file holding an account's Folderstorm setup, exported and imported
later on the same PC or another one, part by part, with an undo.

Aurora already has a backup of its whole settings (Preferences › Language and
updates › Backup, `crates/aurora-viewer/src/settings/backup.rs`): every
setting of the profile, the windows' layout, the skin, the account files. It
is all or nothing per part and has no Merge. This backup is narrower and
sits beside it: the Folderstorm features, section by section, Merge or
Replace, and a safety backup before each import.

---

## 1. What is stored where (on `main`, 2026-11-17)

Everything below lives in the profile's `settings.json` unless said. A
profile is one `--profile` (its own settings folder); several accounts can
log in with one profile.

| Feature | Where | Per | Inventory ids |
|---|---|---|---|
| Layouts, workspaces, favorites, toggle, strip (rows 1.3 to 1.7) | `folderstorm_workspaces` | profile | a workspace's folder windows and HUDs, tagged with their account |
| Background around the world (row 1.8) | `folderstorm_workspaces.background`, a layout's own `background`; pictures in `<settings>/folderstorm_backgrounds/<hash>.<ext>` | profile | none |
| Margin desktop (row 1.10) | `folderstorm_desktop` (switches, shortcuts, taskbar choices) | profile | folder shortcuts, tagged with their account |
| Custom Inventory tabs (rows 2.1, 2.2, 2.13) | `inventory.custom_tabs` | profile | folders and pinned items, by account |
| Inventory windows' pin and quiet mode (row 2.13) | `inventory.window_modes` | profile | folder windows by folder id (no account) |
| Additional Inventory windows (row 2.14) | their places are egui's memory (`ui_layout.ron`), layouts place them | profile | none |
| Gesture boards (row 3.1) | `folderstorm_gesture_boards.accounts[<account>]` | account | every tile |
| World viewport (row 1.1) | `folderstorm_viewport` | profile | none |
| Media choices (top bar's media button, Nearby media) | `media` (Aurora's) | profile | none |
| Session host's characters (row 5.1) | `<main settings>/session-host.json` | viewer | none |
| Chat image upload (row 3.3) | `folderstorm_chat_upload`; token in the credential store | profile | none |
| Local assistant (row 4.1) | `local_assistant`; token in its discovery file | profile | none |
| Other windows behind (row 5.1) | `folderstorm_characters` | profile | none |
| Outfit gallery size, Wear in menus (rows 2.9, 2.7) | `appearance_gallery_size`, `inventory.context_wear` | profile | none |

The task also named "the margin desktop's `folderstorm_desktop.accounts`
(rooms, taskbar)": on `main` the desktop is per profile, with the account on
each folder shortcut; rooms are worked out each frame, not saved.

## 2. Sections

| Id | Label | Merge | What it holds |
|---|---|---|---|
| `layouts` | Layouts and workspaces | yes | the store without its background and without the session's state (`modified`, `pending`, `window_held`); the pictures of the layouts' own backgrounds |
| `background` | Background around the world | no | the default background and its picture |
| `desktop` | Desktop in the margins | yes | switches and shortcuts |
| `inventory_tabs` | Custom Inventory tabs | yes | the tabs with their filters, search, folders and items |
| `inventory_windows` | Inventory windows (pin and quiet mode) | yes | each window's mode |
| `gesture_boards` | Gesture boards | yes | the boards of the account logged in (none before login) |
| `viewport` | World viewport | no | the margins and switches |
| `media` | Media choices | no | Aurora's media settings but the plugins' folder and the download answer (this PC's) |
| `session_host` | Session host characters | yes | each character's slot, profile, alias, colour, standby, sound behind |

**Left out, and why.**

- Local assistant: its permissions open a local API; a shared file must not
  turn them on.
- Chat image upload: its host and upload folders decide where pictures go;
  the same reason.
- The session host's other settings: windows and places of this PC, hotkeys,
  and the controller access (given on each PC).
- Other windows behind (`folderstorm_characters`): a choice for this PC's
  card.
- The outfit gallery size and *Wear* in menus: two plain settings that
  Aurora's own backup already carries; easy to add as a `plain` section if
  wanted.
- The additional Inventory windows' places: egui's memory, carried by
  Aurora's backup and by layouts.

Profile-wide features are written whole: their entries tied to an account
(a workspace's HUDs, a folder shortcut, a tab's folders) keep their account,
so a Replace puts the profile back exactly.

## 3. The file

A plain ZIP (`.zip`, so anyone can look inside):

```
manifest.json          format "folderstorm-account-backup", version 1,
                       product, viewer version, created (local time),
                       kind (manual, before-import, daily), account
                       {id, name}, sections [{id, label, files}]
sections/<id>.json     one section's value
files/backgrounds/<hash>.<ext>   a picture a section names
```

Reading never trusts the file:

- size at most 256 MB, at most 512 entries, each JSON at most 16 MB;
- only what the manifest lists is read; a section id is `[a-z0-9_]{1,40}`;
- a file name is `<folder>/<name>`, the folder one of `file::FOLDERS`, the
  name plain; a background picture's name must be the FNV hash of its bytes
  (as the viewer names its copies) and it is never larger than 32 MB;
- each section is read by the feature's own tolerant reader (the layouts'
  `From<Raw>`, `Desktop::from_value`, the boards' `Account` reader, the tabs'
  `normalize`, `tolerant::from_value`, the host's `Config::cleaned`): limits,
  names and ranges apply as for `settings.json`;
- a newer `version` is read for what this version knows, with a note; an
  unknown section is listed ("left alone") and never offered;
- no `unwrap` on file data.

## 4. No secrets

Never in a backup: passwords, tokens, saved logins, session or capability
URLs, chat or IM logs, e-mail addresses. The sections are written not to hold
them (see what is left out above). Then `secrets.rs` removes, from every
section written and every section read, any key whose name holds
`password`, `token`, `secret`, `api_key`, `mfa`, `credential`, `cookie`,
`session_id`, `capability`, `email`, `login_uri`, `username`, `last_login`…
and any capability URL (`…/cap/…`) or address with a user and password.

Test `no_secret_ever_goes_into_a_backup` fills a profile with sentinel
values in every secret-bearing setting (login name, last login, custom login
URI, MFA hash, GIF keys, the local assistant on, the upload host and folder,
the plugins' folder) and a session host file with the controller access and
a token, then fails if any section's own export (before the guard) has a
denied key or a sentinel, or if any entry of the ZIP holds one.

## 5. Import

1. Import… (file dialog, `rfd`, on a thread) reads the file.
2. The dialog: who and when it is from, the notes (newer version, unknown
   sections, another account, not logged in), a box per section, Merge or
   Replace where Merge is offered, and *Leave out…* when the file points to
   another account's Inventory.
3. Import: the setup as it is now is captured on the frame, then written on
   a thread as a **safety backup** (`before-import`) into the backups
   folder, with the pictures the chosen sections use. If the safety backup
   cannot be written, nothing is imported.
4. Each section is applied on the frame (`backup::frame`, the app's hook),
   the settings saved; the status says what was imported, what was left out,
   and how to undo.

**Merge**: the file's entries are added; one with the same name as an entry
here (layouts, workspaces, tabs, boards: ignoring case) takes its place
(and its id); the others here stay; limits leave the rest out with a note.
A tab keeps what it holds for the accounts the file does not name; a
board's tiles get new ids. A desktop shortcut that is there already is
skipped. Session host characters already listed are kept.
**Replace**: the part becomes the file's.

**Another account.** Item and folder ids mean something for one account
only. When the file's account is not the one logged in, the dialog warns and
*Leave out…* is ticked by default: the folder shortcuts, tab folders and
items, workspace folders and HUDs of other accounts, the folder windows'
modes and the gesture boards are left out (counted in the notes). Unticked,
they are kept, still tied to their own account (the boards go under the
backup's account, never under the one logged in): they show when that
account logs in on this profile. Not logged in: the warning says so, kept
by default.

## 6. Automatic backups

The backups folder is `<settings>/folderstorm_backups/`, names
`<kind>_<YYYY-MM-DD_HHMMSS>_<the account id's 32 hex digits>.zip`.

- A safety backup before every import, always; the last 10 per account are
  kept.
- **Daily automatic backup**: off by default (`folderstorm_backup.daily`);
  when on, once a day per account while logged in (the first frame of the day
  the viewer runs); keeps the last N per account, **N = 5** by default
  (1 to 30, a slider under the switch).
- Preferences lists the newest 40 with *Restore…* (the same dialog, every
  section ticked, Replace) and *Open backups folder*.

## 7. Where the code is

| What | File |
|---|---|
| The module, settings, account | `crates/aurora-viewer/src/folderstorm/backup/mod.rs` |
| The `Section` trait | `folderstorm/backup/section.rs` |
| The sections and their list | `folderstorm/backup/sections/*.rs` |
| The file | `folderstorm/backup/file.rs` |
| The guard | `folderstorm/backup/secrets.rs` |
| The backups folder | `folderstorm/backup/folder.rs` |
| An import | `folderstorm/backup/apply.rs` |
| Threads, the frame hook, the daily backup | `folderstorm/backup/jobs.rs` |
| Demo | `folderstorm/backup/demo.rs` (`AURORA_DEMO_BACKUP`) |
| Tests | `folderstorm/backup/tests.rs` |
| Preferences group and import dialog | `crates/aurora-viewer/src/ui/options/pages/folderstorm_backup.rs` |

Hooks in upstream files (each marked `// Folderstorm:`): `settings.rs`
(the `folderstorm_backup` field and its default), `app.rs` (one call in
`draw_options`), `ui/options/pages.rs` (the module), `ui/options/pages/system.rs`
(the group after Aurora's Backup). The catalogues get an `fs-backup-*`
block after the desktop's. In our own code, `backgrounds/import.rs`'s `hash`
became `pub(crate)`.

The Folderstorm part of Preferences has no page of its own: each feature
hooks a group into the page it belongs to. The backup sits beside Aurora's
Backup on *Language and updates*.

## 8. Adding a section

A feature (for example desktop item shortcuts, custom tab icons, desktop
icon styles, background alert sounds, still on their branches) adds its
part in a few lines:

1. In `crates/aurora-viewer/src/folderstorm/backup/sections/`, a type
   implementing `Section` (`section.rs`):
   - `id()`: a new lower-case id, never changed once released;
   - `label()`: `tr!("fs-backup-section-<name>")`, with en, fr and ja entries
     in the `fs-backup-*` block of the catalogues;
   - `export(&Env)`: `serde_json::to_value` of the feature's settings (or
     `None` when there is nothing, as an account's part before login); list
     the files it names as `"<folder>/<name>"`;
   - `import(&mut Target, Value, &How)`: read the value with the feature's
     tolerant reader, never `unwrap`; follow `how.mode` (Merge by name, or
     Replace); leave out entries tied to another account's Inventory when
     `how.drops(account)` (tagged) or `how.drops_untagged()`; say what was
     left out in `Outcome::notes`;
   - `merges()`: `false` for plain settings;
   - `inventory_refs(&Value)`: how many entries point to an Inventory, if any.
2. Add it to `SECTIONS` in `sections/mod.rs`.
3. If it names files of a new folder (custom icons, sounds), add the folder
   to `file::FOLDERS` with a check of the name and bytes (the background
   pictures check their hash).
4. Extend `full()` in `backup/tests.rs` so the round trip and the secrets
   test cover it; add a Merge case if it merges.

An older viewer reading a file with the new section lists it as unknown and
leaves it alone; the new viewer reading an older file simply does not find
it.

## 9. Left / open

- Tested in the sandbox with unit tests and a demo build; the file dialogs,
  *Open backups folder* and the session host's lock on Windows are for the
  human to try.
- Two backups in the same second get the same name in the backups folder
  (the second replaces the first).
- The account's name goes into the manifest (to say whose backup it is).
  Remove it if the human prefers the id alone.
- A file exported before login has no gesture boards (no account).
