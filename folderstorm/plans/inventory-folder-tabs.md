# Folder-scoped Inventory tabs: plan

Plan for [FEATURES.md](../FEATURES.md) row **2.2**: save an Inventory tab
scoped to chosen folders. It extends the custom Inventory tabs of row 2.1
(`crates/aurora-viewer/src/ui/inventory/custom_tabs.rs`) with a folder scope;
it does not add a second tab system.

## 1. What Folderstorm (C++) does

Branch `codex/inventory-view-folders` (commits `a2df9aca27` "Save inventory
tabs scoped to selected folders" and `45e53a3adc` "Refresh saved folder
ancestry before a tab first opens"):

- **Choosing folders is by selection.** The Inventory gear menu has *Save
  selected folders as view…*: a new custom tab scoped to the folders selected
  in the list, named after the first one, then the Rename prompt. A right
  click on a custom tab adds *Add selected folders to view*, *Use only
  selected folders* and *Show all folders*.
- **What can be chosen**: only folders (one item in the selection disables
  it), never the inventory root, never the Trash or a folder in it, never the
  Library (it must be under the agent's root); at most 32 (a notice says so).
  Disabled in a single-folder window.
- **Scope = the union of the folders and all their sub-folders**, at any
  depth. The folders above a chosen one are shown only as its path, without
  their other contents (`passesViewFolders`: ancestors pass, other entries
  pass only if a chosen folder is among their parents).
- **It combines with the tab's filters and search**: the scope is one more
  condition before the usual filter checks; filter Reset leaves it.
- **Folder ids are kept**, never names. A folder that is deleted, or in the
  Trash, is skipped (`mLiveViewFolders`) but stays in the saved list, so it
  comes back if restored. A moved folder follows (the ancestry is refreshed
  on every structure change, even for a tab not yet opened).
- **Per account**: the custom tabs are saved in `gSavedPerAccountSettings`,
  the folder ids under `view_folders` in each tab's entry. Clone copies them.

## 2. Folderstorm2 design

Where the code goes:

- `ui/inventory/custom_tabs/folders.rs` (new): the scope itself, all pure
  and tested: which folders of a selection may be chosen, the status of a
  saved folder (live, missing, in the Trash, not allowed), add / replace /
  clear, the per-account lists, the live roots handed to the search.
- `ui/inventory/custom_tabs/folder_ui.rs` (new): the menu entries of a tab,
  the *Folders* part of the Customize dialog, the note above the list.
- `ui/inventory/view/folder_scope.rs` (new, a child module of `view` so that
  it reaches the search cache's private fields): narrows the search's folder
  set to the live roots, and lets the scope show outfit folders.
- `custom_tabs.rs` (ours): `CustomTab.folders`, Clone copies it, the tab
  menu and the dialog call into `folder_ui`.

Behaviour (as in Folderstorm (C++) unless marked **differs**):

| Point | Folderstorm2 |
|---|---|
| New tab from folders | Right click on folders › *Create tab* (**differs**: a context-menu entry, as the Legacy style has no gear menu), named after the first folder, Rename dialog opens |
| Add folders to a tab | Right click on folders › *Add to ›* (**adds**): every custom tab by name, then *New tab…*; picking one adds the folders (union, not repeated) and the status line says so; a check mark on a tab that has them all. Dragging folders onto a custom tab's button does the same (**adds**) |
| Several folders | Ctrl+click and Shift+click select them (Aurora's own selection); the menu and the drag take the whole selection |
| Greyed entries | Say why on hover (an item selected, the root, Trash or Library, 32 folders, 10 tabs) |
| Change a tab's folders | Right click on a custom tab › *Add selected folders* / *Use only selected folders* / *Show all folders*; also in *Customize tab…* (**adds**: a *Folders* part listing each folder by path, with a remove button) |
| What can be chosen | Folders only; not the root, the Trash or what it holds, the Library; up to 32 |
| Sub-folders | Included, at any depth; the parents are shown as the path only |
| Filters and search | Apply inside the scope; *Reset* in the filter editor keeps the scope |
| Deleted / Trash | Skipped but kept; a note above the list says how many, with *Remove them* |
| Moved | Follows the folder (ids) |
| Outfit folders | Can be chosen and are shown even when "search outfits" is off (**differs**: Aurora's search hides them otherwise) |
| Storage | `settings.json`, `inventory.custom_tabs[].folders`: a map from account id to folder ids (**differs**: the tabs themselves stay shared by the accounts, as in row 2.1; on an account with no folders saved the tab shows all folders, with a note) |
| RLVa `@showinv` | Nothing new: every entry lives in the Inventory window, which `@showinv` closes |

Upstream hooks (each marked `// Folderstorm:`):

- `view.rs`: `mod folder_scope;`, one field of `ViewCache`, two lines in
  `ViewCache::search` (narrow the scope, widen the outfit rule).
- `inventory.rs`: in `list_scope`, one call that hands the tab's scope to
  the search; in `list`, one call that draws the note above the list.
- `context_catalog.rs`: `mod folder_tabs;` (the entries, in the new file
  `context_catalog/folder_tabs.rs`, a child module so that it reaches the
  menu builder), a `Command` variant, one call after *Review bulk changes*,
  one dispatch line, and a `help` field on the catalog (why an entry is
  greyed). *Add to ›* is a submenu: an existing menu group
  (`Direction::Horizontal`), so the menu layouts and the editor handle it.
- `context.rs`: one line hands the catalog's `help` to the menu, which
  already shows `State::help` as an entry's tooltip.
- `aurora/sidebar.rs`, `aurora/toolbar.rs`: one call each that makes a custom
  tab's button take dropped folders (the Legacy strip is ours).
- `controls.rs`: one call in the filter editor (the *Folders* part).
- `app.rs`: the demo scenario's name and call.

Demo: `AURORA_DEMO_INVENTORY=folder-tabs|aurora-folder-tabs` — a tab scoped
to two demo folders plus one missing, its Customize dialog open.

## 3. Decisions to confirm

1. Context-menu entries *Create tab* and *Add to ›* instead of a gear-menu
   entry; adding does not switch to the tab.
2. Tabs shared by accounts, folders per account (Folderstorm (C++) keeps the
   whole tab list per account).
3. Library folders cannot be chosen (as in Folderstorm (C++)).
4. Outfit folders are shown inside a scope even with "search outfits" off.
5. The limit of 32 folders per tab.

## 4. Preferences search

No Preferences row is added: the feature lives in the Inventory window.
Nothing needs an `fs-search-*` keyword once #69 lands.
