# Inventory quick access: plan

Plan for [FEATURES.md](../FEATURES.md) row **2.13**. The human's goal:
"really, the goal is to be able to easily access common items." It builds on
the custom Inventory tabs (row 2.1) and their folder scope (row 2.2,
[plan](inventory-folder-tabs.md)); it adds no second tab system.

Folderstorm (C++) has none of this: everything below is **new**, so the
behaviour reference is Aurora's own Inventory windows.

## 1. What it adds

1. **Single items in a tab**: pin an item (an outfit piece, a gesture, a
   landmark, an object…) to a custom tab without its folder.
2. **A tab in its own window**: right click on a custom tab › *Open in a
   window*: a small Inventory window that shows just that tab.
3. **Flat list**: a per-tab switch that lists every item of the tab in one
   list, with no folder tree.
4. **For every Inventory window** (main, additional, tab windows, folder
   windows; Legacy and Aurora styles): a **pin** button and a **quiet mode**
   button in the title bar.

## 2. Decisions (to confirm)

### Pinned items

| Point | Decision |
|---|---|
| How | Right click on items (or folders and items together) › *Create tab* / *Add to › <tab>* / *New tab…*, and dragging them onto a custom tab's button: the folders join the tab's scope (row 2.2), the items are pinned. The tab's menu entries *Add selected* / *Use only selected* take items too |
| What can be pinned | An item of the agent's inventory, out of the Trash and the Library (the rule of the folders); up to **64** per tab |
| Links | **The item the link points to is pinned**, not the link: the pin survives the link being removed (an outfit edited, the Current Outfit changing), and the row is the item itself. A link to a folder adds that folder to the scope; a broken link is refused |
| Where they show | **At the top of the tab's list**, before the folders, once each (not also under their folder path, even when a chosen folder holds them), in the window's sort |
| Filters and search | Apply to the pins too, as to the scoped folders: typing narrows the pins |
| A tab of pins only | Shows only its pins (pins scope a tab as folders do: adding one to a tab that showed everything makes it show only what was added, as adding a folder does in row 2.2) |
| Deleted / Trash | Skipped but kept, as folders: the note above the list counts folders and items, *Remove from tab* forgets them all |
| Customize tab… | Its *Folders* part becomes *Folders and items*: each pin by name with its folder path, a remove button |
| Clear | The tab menu's *Show all items* (was *Show everything*, which removed the folders and pins: "that kinda just ruins your custom tab", the human) **keeps the folders and pins** and clears what hides items: the item types, required permissions, links, creator, creation date, *coalesced objects only*, the advanced search's rules, and the search text (a typed search hides items as surely as a type filter); *Also show folders without matching items* and the flat list stay. Greyed when nothing is hidden. Removing every folder and pin at once is *Remove all…* in *Customize tab…* only, after a question (each one still has its own × there) |
| Storage | `inventory.custom_tabs[].items`: account id → item ids, beside `folders` |

### Tab windows

| Point | Decision |
|---|---|
| Open | Right click on a custom tab › *Open in a window*. Again: the window comes to the front (one window per tab) |
| What it shows | That tab only: no tab strip; its scope, pins, filters and search; the usual search box, toolbar and status line (until quiet mode) |
| Mechanism | An entry of `InventoryUi::windows` like Aurora's additional window (same drawing, same dialogs, same clipboard); its state is locked to the tab |
| Size and place | A small window (320 × 420) the first time; then its place and size are kept per tab |
| Aurora style | Always the narrow layout (no shortcuts column, no tab icons): it is a small window |
| Two windows on one tab | They share its filters and search: a change in one shows in the other |
| Title | The tab's name (follows Rename) |
| Tab deleted | Its window closes |
| At login | **Not reopened**, as the additional and folder windows are not. Workspaces (PR #64) remember folder windows; tab windows can be added there later |
| RLVa `@showinv` | Closes them with the main and additional windows |

### Flat list

| Point | Decision |
|---|---|
| Switch | Right click on a custom tab › *Flat list* (a check mark); per tab, shared by the accounts like the tab's filters |
| What it lists | Every item the tab shows (its folders and all below them, filtered), pins first, without folders (*Always show folders* is ignored there) |
| Order | The window's sort |
| Folder path | In the row's tooltip (both styles); also muted at the row's right when the row leaves room, which a full-width row seldom does |

### Pin (every Inventory window)

Aurora's floaters have no pin. The closest meaning is the Folderstorm session
host's pop-out windows (`folderstorm_characters/host/ui/bar.rs`,
`monitor_windows.rs`), whose pin is **always on top**. Inside the viewer this
is: **a pinned Inventory window stays above the viewer's other windows** (it
is brought to the front of the windows' layer at every frame).

- A title bar button, per window: the pin icon, the pinned icon while pinned.
- Remembered per window (main, additional, each tab window, each folder
  window by folder).
- Menus, tooltips and notifications still open above it. Windows it opens
  (Properties, the Rename dialog) are ordinary windows and may open under
  it: move it or unpin. A window clicked comes to the front for one frame,
  then goes back behind it. Being in front, it is the one Ctrl+W closes
  first.
- Not egui's `Foreground` order (tried first): that is the layer of menus
  and of modal questions, and a pinned window there covered a modal question
  (the web and media support offer) that should stay on top.
- Not chosen (could be added): "stays shown when the interface is hidden",
  "Close All Windows (Ctrl+Shift+W) leaves it open".

### Quiet mode (every Inventory window)

A title bar button strips the window down to **the list**.

- **Stays**: the title bar (the title, the pin, the quiet button, minimize,
  close) and the list of the tab or folder shown, with the note above it when
  some of a tab's folders or pins are gone. Right click menus, drag and drop,
  keyboard shortcuts and double-click work as usual.
- **Hidden**: the toolbar, the search box, the tab strip, the filter chips,
  *Customize tab…*, the next-owner rights switch, the status line, Aurora's
  shortcuts column, breadcrumb, column headers and details.
- The search and filters in effect stay in effect: leave quiet mode to change
  them.
- Aurora style: the narrow tree, whatever the window's width.
- Remembered per window, as the pin.

Storage of both: `inventory.window_modes`, a map from a window key (`main`,
`additional`, `tab:<id>`, `folder:<uuid>`) to `{pinned, quiet}`; only windows
not in the default mode are kept, at most 32 folder windows.

## 3. Where the code goes

- `ui/inventory/custom_tabs/items.rs` (new, pure, tested): what a selection
  gives (folders and items, links resolved), the status of a pin, the
  per-account lists, the live pins handed to the search.
- `ui/inventory/custom_tabs/window.rs` (new): tab windows (open, title, lock
  to the tab, close when the tab goes, RLVa).
- `ui/inventory/custom_tabs/flat.rs` (new): the flat list switch and the
  folder path on a row.
- `ui/inventory/window_mode.rs` (new): pin and quiet mode for every Inventory
  window: the title bar buttons, the settings, the quiet list.
- `ui/inventory/view/folder_scope.rs` (ours): the pins and the flat list in
  the search.
- `custom_tabs.rs`, `folders.rs`, `folder_ui.rs`,
  `context_catalog/folder_tabs.rs` (ours): items beside folders, the menus.

## 4. Upstream hooks (each marked `// Folderstorm:`)

- `inventory.rs`: `mod window_mode;`; one field of `InventoryUi`; in `show`,
  the open-a-tab-window request, the title bar buttons of the main window and
  of each window of `windows` (before and after `Floater::show`), the title
  and contents of a tab window; `close_additional_window` also closes tab
  windows; one line at the top of `inventory_contents` and
  `folder_window_contents` (quiet mode); one call in `search_rows` (the folder
  path of a flat row).
- `view.rs`: `ViewCache::search`: the scope check lets the pins through, the
  path walk skips them, one call puts them on top (and makes the flat list).
- `view.rs` `InventoryPreferences`: one field, `window_modes`.
- `aurora.rs`: in `contents`, the layout (narrow for a tab window or quiet
  mode) and a quiet arm; in `compact`, no tab icons in a tab window.
- `context_catalog.rs`: *Create tab* / *Add to ›* on items too (the existing
  hook line loses its folders-only condition).
- `app.rs`: the demo scenario's name and call.

## 5. Demo

`AURORA_DEMO_INVENTORY=quick-access|aurora-quick-access`: a tab "Everyday"
scoped to the demo's Body folder with two pinned items (one through a link)
and a deleted pin, shown in the main window, and opened in its own window,
pinned and quiet. `AURORA_DEMO_QUICK_ACCESS=flat`: the tab in a flat list.

## 6. Preferences search

No Preferences row is added: everything lives in the Inventory windows.
Nothing needs an `fs-search-*` keyword once #69 lands.
