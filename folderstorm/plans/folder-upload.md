# Folder-preserving bulk upload (FEATURES 2.11)

Upload a folder of files from disk and recreate its folder tree in
Inventory, after a preview that shows the total L$ cost and asks to confirm.
Branch `claude/folder-upload`.

## What Folderstorm (C++) does

Branch `codex/sl-folder-upload` (`63d11dbc1e`, with the review corrections
of `6eef8a47ae` and `doc/plans/upstream-inventory-upload-review.md`):

- **Entry points**: *Upload › Folder…* in the Inventory's **+** menu
  (`menu_inventory_add.xml`, the upload goes into the folder the menu was
  opened on) and in the main *Upload* menu (into the inventory root).
- **Scan** of the chosen directory on a worker: at most 10,000 entries and
  32 levels; symbolic links and reparse points skipped; the root itself must
  be a real folder.
- **Cost and confirmation** through the native bulk-upload floater
  (`upload_bulk` → `LLFloaterBulkUpload`): number of files, total L$, the
  2K-texture option, the balance check, Upload / Cancel. Unsupported files
  are left out and announced there.
- **Run**: one new folder named after the disk folder under the destination,
  then one new inventory folder per sub-folder (empty ones included). All
  folders are created first, one request at a time; a failed folder stops
  everything before any file is sent ("No files were uploaded. N folder(s)
  were created and remain in inventory"), with no retry. glTF/GLB files go
  into new `Materials` / `Textures` sub-folders. Files are then handed to
  the native bulk uploader, folder by folder.
- **Changes after confirmation**: files are re-checked (size, date) before
  the run; a changed file cancels with "select the folder again to refresh
  its upload cost".
- **Session**: a logout, a disconnect or another account cancels silently.
- **Names**: new folders are always created (never merged into an existing
  folder of the same name); duplicate item names are allowed, as on the grid.

## What Aurora already has

Upstream's unified uploader (`crates/aurora-viewer/src/imports/`, Ctrl+U,
docs/IMPORTS.md) already does most of this: **Add folder** scans a directory
on a worker (`imports::folders::discover`: links and reparse points skipped,
100,000 entries, 64 levels, 256 files per selection, 512 drafts, 1 GiB), the
**Folder preview** tab shows the inventory tree that will be created, with
three arrangements (*mirror*, by type, single folder) and *Include source
folder*; prices come from the account's benefits; the Upload button needs
enough L$, and a confirmation dialog shows the total. Folders are created
only after confirmation, one acknowledged request at a time, then files go
through the paid queue with per-file progress, errors, cancellation, no
automatic retry of an unknown outcome, and the receipt's L$ total.
Sources are decoded and kept in memory when added, so a file changed on
disk after the preview cannot change what is uploaded or its price.

So the port is **not** a new uploader. What is missing is Folderstorm's
one-step flow: today a user has to open the uploader, untick *Use default
folders*, choose *mirror*, pick the destination, then *Add folder* — and the
choice stays in their settings afterwards.

## What we build

1. **Inventory entry point** — *Upload a folder here…* in the Inventory's
   **+** (New) menu of a folder, as Folderstorm (C++) does. It picks a disk
   folder, then opens the uploader on its **Folder preview** tab with the
   batch routed as a mirror of the disk folder into that inventory folder
   (*Use default folders* off, *mirror*, destination = the folder,
   *Include source folder* on).
2. **Routing for this upload only** — the user's previous routing settings
   are remembered and put back once the uploader is closed with nothing
   running, unless the user changed them in the meantime (then their choice
   stays). Folderstorm (C++) never touched settings.
3. **Preview in the confirmation** — the existing confirmation dialog gains a
   Folderstorm block while a folder upload is the current batch: source
   folder → inventory path, files by type, new folders, the files that will
   **not** be uploaded (unsupported types, with their names), name warnings
   (a folder of the same name already in the destination: a second one is
   created; two files that will get the same item name), and the balance
   before / after. The total L$ line stays upstream's.
4. **Receipt** — when the batch ends, one notice: files uploaded, failed,
   cancelled, not sent, and L$ spent.
5. **Core usable by a later MCP tool** (not built now; MCP work is paused):
   `folderstorm::folder_upload` exposes `start_with_path(destination, path)`
   (no dialog), `Preview::build(…)` (plain data) and `Receipt` (per-file
   rows). An MCP tool would call start → preview → (user confirms in the
   viewer) → receipt, without its own copy of the uploader.

### Where the code goes

- `crates/aurora-viewer/src/folderstorm/folder_upload/` — `mod.rs` (state,
  routing, start / frame), `plan.rs` (pure: skipped-file scan, preview,
  name collisions, receipt), `ui.rs` (confirmation block, notice text),
  `demo.rs` (fixtures), tests.
- `crates/aurora-viewer/src/app/folderstorm_folder_upload.rs` — the `App`
  glue (picker job, routing, demo).

### Upstream hooks (each one marked `// Folderstorm:`)

- `app.rs`: `mod` line, one `App` field and its default.
- `app/imports.rs`: one call per frame in `show_imports`.
- `app/inventory.rs`: one match arm for the new inventory action.
- `ui/inventory.rs`: one `InvAction` variant.
- `ui/inventory/context_catalog.rs`: one menu entry in the **+** group.
- `imports/files.rs`: `add_folder_path`, a public door to the existing
  private loader for a folder chosen elsewhere (the dialog and the MCP path).
- `imports/ui.rs`: one call in the confirmation dialog.
- `folderstorm/mod.rs`, locales (en/fr/ja), `docs/TESTING.md`.

### Demo

`AURORA_DEMO_FOLDER_UPLOAD=preview|confirm|upload` builds a small disk tree
(textures and a sound in nested folders, an empty folder, an unsupported
`.txt`), starts the flow on an inventory folder without a dialog;
`confirm` opens the confirmation dialog with the Folderstorm block;
`upload` confirms and runs it on the offline demo server, then shows the
receipt notice. Listed in `docs/TESTING.md`.

### RLVa

No RLVa restriction covers uploads in the Legacy Viewer / RLVa. The entry
point lives in the Inventory, which `@showinv` already hides. Nothing to add.

## Decisions to confirm (picked closest to Folderstorm (C++) where it mattered)

1. **Empty disk folders are not created** in Inventory (Aurora's importer
   plans folders from files; Folderstorm (C++) created every folder).
   Changing that is a deep change to upstream's planner.
2. **glTF/GLB keep Aurora's routing** (a material and its textures beside
   it, shared textures uploaded once), not Folderstorm (C++)'s `Materials` /
   `Textures` sub-folders.
3. **Aurora's limits stay** (256 files per selection, 512 drafts, 1 GiB,
   100,000 entries, 64 levels) instead of 10,000 entries / 32 levels.
4. **Errors before the run block the upload** (Aurora: a row in error, such
   as a BVH, must be removed or deselected before Upload), where
   Folderstorm (C++)'s floater left unsupported files out on its own.
   Unsupported *types* are skipped and listed, as in Folderstorm (C++).
5. **Other drafts already in the uploader** are kept and follow the same
   routing; they appear in the preview before anything is confirmed.
6. **Main-menu entry**: none added. Aurora's quick import bar already has
   *Folder* (into the routing shown in the uploader); the new entry point is
   the Inventory's, with a destination.

## What is left after this branch

- Grid test by the human (cost shown vs. charged, folders and items created).
- Drag and drop of a disk folder onto an Inventory folder (not in
  Folderstorm (C++); possible later through the same `start_with_path`).
- MCP file-upload tools (parity group 8) on top of this core.
