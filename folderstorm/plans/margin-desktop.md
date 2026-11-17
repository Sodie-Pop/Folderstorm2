# Margin desktop and taskbar: plan

Plan for [FEATURES.md](../FEATURES.md) row **1.10**. Branch
`claude/margin-desktop`. **Checkpoint 1 is built** (command shortcuts, the
desktops, Preferences, the demo), and **checkpoint 2** on
`claude/margin-desktop-folders` (folder shortcuts), and **checkpoint 3**
on `claude/margin-desktop-cp3` (editing, the toolbar, the keyboard);
checkpoint 4 (the taskbar) and checkpoint 5 (item shortcuts) are not. What
they changed against this plan is in sections 19 to 22.
The human's request (2026-11-16):

> On a 2nd or 3rd monitor, which we've been referring to as margins (where
> we recently worked on a background image fix), I'd like a big feature
> added. The idea is to use the space similar to a PC desktop with the
> ability to set shortcuts for specific inventory folders as well as
> anything that could normally be opened with a toolbar button. I'd also
> like the option to change the behaviour of a collapsed window to display
> them along the bottom of that monitor like a Windows taskbar.

It builds on the custom world viewport (row 1.1), the monitors' list (row
1.2), the margins' background (row 1.8, and its fix, PR #78) and the
Inventory folder windows. It adds no second toolbar and no second window
system: a shortcut runs what Aurora already runs, and the taskbar shows
Aurora's own windows.

Line numbers below are those of `main` at `f18c3288` with PR #78 merged in
(`claude/fix-workspace-backgrounds`, `9e9e36d1`). Paths are under
`crates/aurora-viewer/src/` unless they start with `crates/`.

The FEATURES row says what it does in one paragraph.

## 1. What it adds

1. **A desktop on each margin monitor.** Each monitor under the viewer
   window that is not wholly the world gets a grid of shortcuts, over the
   margins' background (row 1.8) and under every window.
2. **Folder shortcuts.** A shortcut to one Inventory folder: opening it
   opens that folder's own window (*Open in a new window*, Aurora's
   `open_folder_window`), or brings it to the front when it is open.
3. **Command shortcuts.** A shortcut to any command of the toolbar
   catalogue (81 today): opening it does what the toolbar button does.
4. **Arranging.** Drag a shortcut to another cell or another monitor; it
   snaps to the grid. Rename, choose an icon, remove.
5. **A taskbar** (optional): an island along the bottom of one margin
   monitor with a button per collapsed window (or, by a switch, per open
   window). Click: restore and bring to the front. A collapsed window is no
   longer drawn as a title bar: its button stands for it.

**In v1:** folders and commands (checkpoints 1 to 3), the taskbar
(checkpoint 4). **Not in v1:** single items (landmarks, notecards, outfits:
section 2, "Items"), files of the computer, web links, widgets, a free
(unsnapped) placement, a desktop of its own per layout.

## 2. What Folderstorm (C++) had

Nothing of this. Checked on 2026-11-16 in its public docs (fetched from
`raw.githubusercontent.com/DrCum/folderstorm/main`): `README.md`,
`doc/help.md`, and `doc/plans/` `workspace-enhancements.md`,
`complete-workspaces.md`, `visual-viewport-setup.md`,
`workspace-context-and-snapshot.md`. No desktop, shortcut icon, launcher or
taskbar in any of them. The only neighbour: a workspace saved each
window's "minimized state" (`complete-workspaces.md`, lines 35 to 40), which
our layouts already do (`workspaces/store.rs` `Placement::minimized`). The
other files of `doc/plans/` were not listed (the GitHub API is closed to
this session), and its code was not read.

So the behaviour reference is Aurora's own toolbar, Inventory windows and
floaters, and Windows' desktop and taskbar for what the human named.

## 3. What Aurora and the fork already have (verified in code)

| What | Where | What it means for us |
|---|---|---|
| The renderer draws what precedes the end-of-labels marker as world labels, **cut to the world viewport**; the rest of the frame is the interface | `crates/aurora-render/src/ui_world.rs:19` (`end_world_labels`), `:28` (`take_end`); `app.rs:16743` | Anything of ours painted before the marker never shows in a margin (the bug of PR #78) |
| The margins' background is spliced into the frame's shapes where its own mark was left, right after that marker | `folderstorm/backgrounds/paint.rs:230` (`mark`), `:254` (`underlay_in`); `app.rs:16744`, `app.rs:12172` | The wallpaper is on the background layer, first. The desktop must come after it and after the marker |
| `LayerId::background()` is registered at the start of every pass, and never asks to be on top; areas of `Order::Background` are sorted after it | egui 0.37 `context.rs:549`, `memory/mod.rs:1335` and `:1411`, `layers.rs:236` | An `egui::Area` of `Order::Background` is painted after the whole background layer (so after the marker and the wallpaper), and before every window (`Order::Middle`). Aurora uses such areas already (`ui/chat.rs:3925`, `app/busy_pause.rs:135`) |
| The margins and the monitors' parts of the window, in points | `folderstorm/backgrounds/place.rs:15` (`margins`), `:34` (`areas`); `folderstorm/viewport.rs:220` (`world_area`), `:313` (`monitors`) | The desktop's rooms come from the same maths. No monitor list on Wayland: one area, the window |
| No margins outside the world screen, in a photo taking the whole window, or in mouselook when the viewport is dropped there | `app/folderstorm_viewport.rs:16` (`viewport_overrides`) | No desktop on the login screen, by construction |
| A click the interface does not take goes to the world. "Takes" is `is_pointer_over_egui()`, which is **false over the background layer and over an `Order::Background` area** in a margin | `app/click_pick.rs:289` (`ui_takes_pointer`), `ui/widgets.rs:76` (`pointer_over_ui`); egui `context.rs:3442` | A desktop shortcut needs one hook there, or a click on it also presses the world (a pick outside the world hits nothing, `aurora-render/src/main_thread.rs:1061` `to_world`, but the press still takes the keyboard from the fields and the wheel still zooms) |
| The toolbar catalogue: 81 commands, each an id, a Tabler icon, an i18n key, `ready` | `ui/toolbar/catalogue.rs:29` (`COMMANDS`, re-exported `ui/toolbar.rs:15`) | The list of command shortcuts, their icons and names. No hook to read it |
| One function runs a command: toggles a `Panels` flag, or returns a `BarAction` for the app | `ui/bars.rs:2361` (`run_command`); the app's `match a.bar` at `app.rs:14788` | A command shortcut calls it and hands the action over, as a toolbar button does (`ui/bars.rs:3376`) |
| Three gates sit in the toolbar's row, not in `run_command`: not `ready`; nothing to do now (Photo roll empty, no Destination Guide on this grid); the Sit button locked by RLVa | `ui/bars.rs:3252`, `:3300`, `:3329`, `:3375`; `rlva_sit_locked_id` `:2539` (`pub(crate)`); `ui/toolbar.rs:272`, `:275` | We apply the same three (section 5, "Commands") |
| Three buttons own a pop-up anchored to the button instead of a command: Favorites (grid), VR view (modes), AO (quick panel) | `ui/bars.rs:3334` to `:3373` | Favorites and VR view cannot be shortcuts in v1; AO opens its window (`run_command`'s own arm) |
| RLVa closes Inventory, the mini-map and the world map every frame while restricted | `app/rlva.rs:141` to `:150` | A command shortcut cannot open them past RLVa |
| A toolbar button being dragged carries a private payload | `ui/toolbar.rs:211` (`Drag`), `:463`, `:473` (`dragging`) | One small hook to read the command dragged |
| The Inventory's drag carries ids anyone may read; a destination gives its cursor | `ui/inventory/drag.rs:128` (`carried_ids`), `:134` (`feedback`) | Drop from Inventory with no new hook. Precedent: `folderstorm/gesture_boards/window.rs:657` (`inventory_drop`) |
| The world's own Inventory drop runs after the interface's pass, and reads a ray that is None outside the world | `app.rs:11935` (`inventory_drag_input`), `app/inventory_drag.rs:299` | A drop on the desktop is taken first (payload cleared), nothing reaches the world |
| A folder's own window, listed and opened by folder, and its `@showinv` refusal | `ui/inventory.rs:727` (`open_folder_window`); `ui/inventory/folderstorm_windows.rs:19`, `:29`; `app/folderstorm_workspaces.rs:164` | The folder shortcut's action exists. One window per folder is our rule to add |
| Every Inventory window has a pin and a quiet mode, kept per folder for folder windows | `ui/inventory/window_mode.rs` (row 2.13) | A folder shortcut plus quiet mode is a small folder pop-up, for free |
| The right-click menu of folders and items is one catalogue for both Inventory styles; our entries are already hooked in | `ui/inventory/context_catalog.rs:765`; `ui/inventory/context_catalog/folder_tabs.rs:28` | *Add to desktop* is added in our own file |
| A collapsed floater is a title bar 200 points wide, where the window stood. **There is no row or tray of minimized windows** | `ui/widgets.rs:245` (`MINIMIZED_W`), `:553` to `:557`, `:654` | The taskbar is new |
| Public helpers: minimize, expand, bring to front, close "as its cross does", is minimized, is front | `ui/widgets.rs:1340`, `:1412`, `:1395`, `:1448`, `:1404`, `:1374` | The taskbar's clicks need no new API |
| A floater kept hidden returns early from `show`, after telling its cascade group it is hidden | `ui/widgets.rs:532` to `:546` (`window_hidden`) | The model for "collapsed and in the taskbar" |
| A cascade group forgets a window it has not heard of for two frames, and then places it again as new | `ui/widgets/cascade.rs:89` (`retain`) | A held window must call `cascade::hidden` each frame, or a notecard or a torn-off conversation jumps when it is restored |
| A window with a compact body when minimized (Performance) is not collapsed | `ui/widgets.rs:355`, `:557` | The taskbar never holds those |
| The bars' islands are areas of the windows' layer, and windows keep their title clear of them | `ui/bars/islands.rs:399` (`area`); `ui/layout.rs:364` (`island_drawn`), `:418` (`clear_of_islands`). Precedent: `folderstorm/workspaces/strip.rs:284` | The taskbar is an island: Aurora's glass, the pointer taken, titles kept clear, with no hook |
| Hidden interface and immersive mode hide the windows before anything else in `show` | `ui/widgets.rs:1150` (`hide_windows`), `:1167` | Windows hidden that way are not seen by the taskbar |
| A layout saves each window's place, size, open and minimized, read from egui's memory | `folderstorm/workspaces/capture.rs:105` to `:122` | Still right for a window held by the taskbar (its place is the last one drawn, its size the expanded one) |
| Preferences › Layout › Bars and windows already calls our group | `ui/options/pages/layout.rs:11` | Our settings rows need no hook |
| The fork's keyboard actions are one enum of ours | `folderstorm/keys.rs` | New actions need no hook |

## 4. Definitions

- **Margin**: the viewer window less the world viewport (row 1.1).
- **Slot**: the n-th monitor under the viewer window, counted left to
  right, then top to bottom (`place::areas`, sorted). Without a monitor
  list (Wayland): one slot, the window.
- **Room**: the part of a slot a desktop is laid in: the slot less the
  world, less what the bars keep, less the taskbar. A slot wholly under the
  world has no room.
- **Desktop**: the shortcuts of one slot, on a grid of cells.
- **Shortcut**: a folder or a command, with its cell, and optionally a name
  and an icon of its own.
- **Taskbar**: the island of window buttons at the bottom of one room.
- **Held**: a collapsed window that the taskbar shows instead of its title
  bar.

## 5. Decisions (to confirm)

Each line gives the default the build starts from.

### The desktop

| Point | Decision |
|---|---|
| On / off | A switch *Shortcuts in the margins*, **off by default** (as the background and the favorites strip). The first *Add to desktop* from a menu turns it on and says so |
| Where | One desktop per slot that has a room. The world's own monitor has one only where the world leaves a strip at least one cell wide |
| No margin at all | Nothing is drawn, nothing is lost: the shortcuts are kept for when the window is stretched again |
| Shortcuts on a slot that has no room now | Kept, not shown. Preferences says "N shortcuts are on a monitor that is not in use" with *Bring them here* (moves them to the first room) |
| Grid | Cells of one size, from the room's top-left corner, filled down then right (Windows' order). Three sizes: Small (icon 24, cell 76 × 68), Medium (32, 92 × 84, default), Large (48, 112 × 104) |
| Snapping | Always: a shortcut stands in a cell. No free placement in v1 |
| A cell under a bar, the world or the taskbar | Blocked. A shortcut stored there is shown in the nearest free cell, and its stored cell is kept (it goes back when the cell is free) |
| Limits | 256 shortcuts for an account; a name of 64 characters |
| Look | A Tabler icon over a name of two lines at most, cut with "…". A folder: `folder-filled`, tinted with its Aurora folder colour when it has one (`ui/inventory/folder_colors.rs:32`). A command: the catalogue's icon. The hovered and the selected shortcut get the theme's pill (`ui.visuals().widgets.inactive.corner_radius`, `p.raised`), the name is drawn on a discreet `p.panel` pill so that it reads over any wallpaper. Palette only, no radius or colour written by hand |
| Tooltip | A folder: its path in the Inventory. A command: the catalogue's label, and why it is greyed when it is |

### Shortcuts: what can be one

| Point | Decision |
|---|---|
| Folders | A folder of the agent's own inventory, out of the Trash and of the Library (the rule of the folder tabs, row 2.2). A link to a folder gives the folder |
| Commands | Every command of the catalogue that is `ready`, but **Favorites** and **VR view** (their button is a pop-up anchored to the button). A command that is not ready is refused with Aurora's "Not available yet" |
| Items | **Not in v1.** Each kind opens its own way (a double click wears clothing, plays a gesture, opens a notecard in a window owned by an Inventory window), and outfit safety (row 2.7) asks before a folder is worn. Cheapest to add later: landmarks (`InvAction::TeleportLandmark` exists). To decide after v1 |
| The same thing twice | Allowed on different slots, refused in one desktop (the existing one is selected instead) |
| A folder that is gone (deleted, or put in the Trash) | The shortcut stays, greyed, with "This folder is no longer in your inventory" and *Remove* |
| Another account's shortcuts | Never shown: each account has its own desktop (section 6) |

### Adding

| From | How |
|---|---|
| Inventory, right click on folders (both styles) | *Add to desktop* (greyed with the reason for an item, the Trash, the Library). Goes to the first free cell of the first room |
| Inventory, drag | Drop folders on a desktop: the room is outlined (`p.violet_light`, or `p.danger` when nothing of the cargo can be added), the cursor says Copy or Not allowed, the shortcut lands in the cell under the pointer. Items of a mixed selection are left out and counted in a short notice |
| A toolbar button, right click | *Add to desktop* |
| A toolbar button, or a command of the *Toolbar buttons* window, drag | Drop on a desktop: a shortcut (the button stays on its toolbar: a copy, not a move). Toolbars must be unlocked to drag a button, as today |
| The desktop, right click on an empty cell | *Add a command ›* (the catalogue sorted by name, in a scrolled menu), *Toolbar buttons…* (opens Aurora's window to drag from), *Arrange by name*, *Icon size ›*, *Desktop settings…* |

### Arranging and editing

| Point | Decision |
|---|---|
| Move | Drag a shortcut: a ghost follows the pointer, the target cell is outlined, release drops it. Onto a taken cell: the two swap. Onto another slot's room: it moves there |
| Select | A click selects one shortcut. Several at once (Ctrl, a rubber band): later |
| Rename | Right click › *Rename*, or F2: the name is edited in place; Enter keeps, Esc cancels. An empty name gives the folder's or the command's own name back. A custom name never renames the folder |
| Icon | Right click › *Icon ›*: a short list of Tabler icons (about 40, chosen for folders: clothes, body, hair, home, tools, heart, star…), and *Default*. A folder's colour comes from its Aurora folder colour; a colour of its own: later |
| Remove | Right click › *Remove from desktop*, or Delete on the selected one. No question: only the shortcut goes |
| Arrange | *Arrange by name* fills the room again from its first cell |
| Folder shortcut, more | *Show in Inventory* (the main window on that folder), *Open in a new window* (the default action) |

### Opening

| Point | Decision |
|---|---|
| Click | **Double click opens**, a single click selects. Some commands act at once (Teleport home, Sit, Stop animations, Photo to disk): a stray click must not run them. A switch *Open with a single click* for those who want the toolbar's feel |
| A folder | Its own window. Already open: it comes to the front (expanded when it was collapsed). Opened for the first time from a shortcut: placed in that shortcut's room, beside it |
| A command | `run_command`, then the `BarAction` to the app, exactly the toolbar button's path. A window command toggles: open closes, as its button does |
| Lit while open | No in v1 (the toolbar's own state is private to `ui/bars.rs`); the taskbar shows what is open |

### The taskbar

| Point | Decision |
|---|---|
| On / off | A switch *Taskbar in a margin*, **off by default**. Works without the shortcuts and the other way round |
| Where | Along the bottom of one room: *Automatic* (the widest room; the leftmost of two equal ones) or a chosen slot. An island of the dock's height (`ui/bars/islands.rs` `HEIGHT`), 8 points from the room's edges |
| What it lists | **The collapsed windows only**, in the order they were collapsed: the closest to the request's words ("change the behaviour of a collapsed window to display them along the bottom of that monitor"). A switch *All open windows* lists every open floater in the order they were opened (Windows' model), which gives the active highlight and click-to-collapse a meaning (decided by the main session on 2026-11-16, the human asleep; open question 1) |
| A button | The window's title-bar icon and its title, cut with "…" (tooltip: the whole title). A window with something waiting (Conversations) wears Aurora's attention mark (`widgets::attention_mark`). With *All open windows*: the front window is lit, a collapsed one is muted |
| Click | Collapsed: restore and bring to the front. With *All open windows* — in front: collapse; otherwise: bring to the front |
| Right click | *Restore* / *Collapse*, *Close* (as the window's cross: its own questions run). Middle click: *Close* |
| A collapsed window | **Held**: not drawn, its button stands for it. Its place and size are kept; restored, it is where it was |
| Too many | Buttons narrow down to the icon alone, then the rest go in a *More* menu at the right end (the favorites strip's way) |
| Above the windows? | **No, in v1**: it is an island like Aurora's dock. A window dragged over it covers it, a click on it brings it back, and windows keep their title bar clear of it. *Keep the taskbar above the windows* (raised each frame, as a pinned Inventory window is): later, if wanted |
| No margin, or the chosen slot has no room | No taskbar, and collapsed windows are Aurora's title bars again, where they stood. Nothing to configure |
| Windows not listed | Those with a compact body when minimized are listed but never held (Performance: its "minimized" is a compact view). Dialogs and the mini-map are not floaters: not listed |

### Hidden when

| State | Desktop | Taskbar |
|---|---|---|
| Login, loading, the full-frame photo view | No margin: nothing | Nothing |
| The window on one monitor (the world fills it, or margins under a quarter) | Nothing | Nothing; title bars as in Aurora |
| Interface hidden, immersive mode | Hidden | Hidden; the windows are hidden by Aurora, and come back as they were |
| Mouselook with the viewport kept | Hidden, as the bars are (shown when Aurora's *Show the interface in mouselook* is on) | Hidden; held windows stay held (they do not pop back as title bars) |
| Mouselook with the viewport dropped | No margin: nothing | Nothing; held windows stay held while in mouselook |
| A modal question open | Drawn, inert (the modal takes the pointer) | The same |

### RLVa and creators' permissions

| Point | Decision |
|---|---|
| `@showinv` | A folder shortcut does not open (as a workspace's folder windows, `app/folderstorm_workspaces.rs:164`). **Stricter than needed:** its name and folder path are hidden too while the restriction lasts (a lock icon, "Inventory folder", tooltip "Blocked by RLVa"). Adding folders is refused |
| Commands | The same path as the toolbar button, plus the row's three gates. So Inventory, the mini-map and the world map are closed again by `app/rlva.rs`, Sit is greyed while RLVa would refuse it, and teleports and building go through the app's own checks |
| Permissions | A shortcut keeps a **folder id** (later an item id), never an asset id, and nothing read from an asset. Its name is read from the inventory when drawn; only a name the user typed is saved. Nothing is copied, exported or shown that the Inventory window does not show. No new rule is needed in `world::inventory::actions`; if item shortcuts come, they go through `actions::agent_mask` like the gesture boards |
| Text from others | A folder's name is shown as text only, never read as a command |

### Layouts and workspaces

| Point | Decision |
|---|---|
| Does a layout carry its desktop? | **No, in v1.** One desktop set for each account, keyed by slot: it follows the monitors, not the layout. A layout that moves the world onto a slot hides that slot's desktop until the world leaves |
| What layouts already do | They save and restore each window's collapsed state, so a layout applied with the taskbar on fills the taskbar |
| Later | A layout's own choices, as for the background: *Shortcuts: default / hidden*, *Taskbar: default / off / slot N* |

## 6. Data model and storage

`settings.json`, field `folderstorm_desktop`, beside `folderstorm_workspaces`.

```
folderstorm_desktop: {
  shortcuts_on: bool,          # absent = off
  size: "small" | "medium" | "large",
  single_click: bool,
  taskbar: { on: bool, slot: null | u8, all_windows: bool },  # absent = collapsed windows only
  accounts: {                  # one desktop for each account
    "<account uuid>": {
      next_id: u32,
      shortcuts: [
        { id, target: { folder: <uuid> } | { command: "<id>" },
          slot: u8, col: u16, row: u16,
          name?: string, icon?: string }
      ],
      kept: [ ... ]            # entries this version cannot read, written back
    }
  },
  # only in a file of checkpoints 1 and 2, until an account takes them:
  next_id, shortcuts: [ ... target: { folder, account } | { command } ... ], kept
}
```

- **Each account has its own desktop** (the human's decision, section 21;
  as the gesture boards and the folders of a custom tab): the shortcuts,
  their places, their names and icons are kept under the account's id, and
  only those of the account logged in are read by the surface. Another
  account's shortcuts never show, greyed or not; switching back shows the
  first account's desktop as it was. At the login screen nothing shows.
- **The profile's choices**: the switch (*Shortcuts in the margins*), the
  icon size, `single_click` and the taskbar's choices, as every other
  setting of Preferences. The size is the profile's because it fits the
  monitors, which the accounts of a profile share, and because a shortcut
  keeps a cell, not a position: an arrangement holds at every size.
- **A file of checkpoints 1 and 2** (the shortcuts beside the choices) is
  read: a folder shortcut goes to the account it named, the commands (and
  what could not be read) to the first account that logs in; the settings
  are saved at that moment. A shortcut keeps its id unless that account
  has it already. Until an account logs in, they are written back as they
  were read.
- **Ids only.** Folder ids and command ids. No asset id, no inventory
  name, no window title.
- **Reading never fails and never drops**: an entry that cannot be read (a
  newer version's target, a damaged file) is kept as read and written back
  (`gesture_boards/store.rs`'s `kept`). Values out of range are brought
  back in. A command id the catalogue no longer has is shown greyed ("This
  command no longer exists") until removed.
- **Kept by "Reset everything"** (`settings.rs:3296`
  `reset_keeping_account`: user content, as the layouts and boards).
- **In settings backups** with the rest of `settings.json`.
- **Saved** when a change ends (the pointer is up), as the boards are.
- **Not saved**: the taskbar's content (it is the windows that are open),
  the selection, the collapsed state (Aurora's `ui_layout.ron` has it).

## 7. Drawing order and input

**Order, back to front:** the world → the world's labels (cut to the
world) → the end-of-labels marker → the margins' background (row 1.8) →
HUD labels, the Legacy bars' panels (all on egui's background layer) →
**the desktops** (one `egui::Area` of `Order::Background` per room) →
windows and the bars' islands, **the taskbar among them** (`Order::Middle`)
→ menus, tooltips.

- The desktops are drawn from one call placed **after the bars** (the work
  area and the islands of the frame are known) and before the windows.
  Being areas of `Order::Background`, their shapes come after the whole
  background layer whatever the call's place, so after the marker. A frame
  test cuts the labels as the renderer does and checks the shortcuts still
  show (the test of PR #78, `backgrounds/tests.rs:184` `world_labels_cut`).
- A room never overlaps a bar: the Legacy bars are painted on the
  background layer, under our areas, so the room leaves out what the bars
  keep (`viewport::bars_screen`, `layout::work_area`, the session host's
  strip), for the three places of the bars (inside the world, around it,
  over the window).
- **Clicks.** One hook in `ui_takes_pointer`: the pointer is ours when the
  topmost layer under it is a desktop's (`ctx.layer_id_at`). So a press, a
  release or the wheel over a desktop never reaches the world, and a
  window over the desktop still wins. The taskbar needs none: an island
  already counts as interface.
- **The whole room takes the pointer**, not only its shortcuts: a right
  click on an empty cell opens the desktop's menu, and a margin has no
  world to click anyway.
- **Hover**: the pill and the tooltip, through egui's own responses.
- **Drops** are read inside our call, before the world's
  (`inventory_drag_input` runs after the interface's pass), and the
  payload is cleared.
- **Requests.** The areas return what was asked (run a command, open a
  folder, save). The app's glue runs them: `bars::run_command` with
  `&mut self.panels`, the result into `a.bar` (`app.rs:19336`, one slot,
  as the toolbars fill it); a folder through
  `inventory_ui.folderstorm_open_folder`.
- **The taskbar reads the windows of the last frame**: each floater tells
  it, from the hook in `Floater::show`, its id, title, icon, collapsed and
  attention. One frame late, never seen. The same call answers "am I
  held?".

## 8. Where the code goes

All new, ours:

- `folderstorm/desktop/mod.rs`: the frame call, `takes_pointer`, the
  requests.
- `folderstorm/desktop/store.rs` (pure): the record of section 6, limits,
  lenient reading, add / move / swap / rename / remove / arrange.
- `folderstorm/desktop/place.rs` (pure): slots, rooms for each place of
  the bars, cells, blocked cells, the nearest free cell, the taskbar's
  strip. Uses `backgrounds::place::{margins, areas}`.
- `folderstorm/desktop/targets.rs` (pure): what a folder or a command
  gives (name, icon, greyed and why), the folder rule, the command table
  (every catalogue id is classed: runs, pop-up only; a test compares it
  with `COMMANDS`, so an upstream command added at a sync is looked at),
  the three gates.
- `folderstorm/desktop/surface.rs`: the areas, a shortcut's face, hover,
  selection, the drag between cells, drops from Inventory and from a
  toolbar, the menus, the rename field.
- `folderstorm/desktop/taskbar.rs`: the windows seen, `holds`, the island,
  its buttons, overflow, the menus.
- `folderstorm/desktop/demo.rs`, `folderstorm/desktop/tests.rs` (and
  `tests/`).
- `app/folderstorm_desktop.rs`: the glue (facts from the world: the
  inventory, the account, RLVa, the interface's state; running the
  requests; saving; the demo).
- `ui/options/pages/folderstorm_viewport/desktop.rs`: the Preferences
  group, a child of our page.
- `ui/inventory/context_catalog/folder_tabs.rs` (ours already): the *Add
  to desktop* entry.

## 9. Upstream hooks (each marked `// Folderstorm:`)

| File | About line | Change | Checkpoint |
|---|---|---|---|
| `folderstorm/mod.rs` (ours) | 7 | `pub mod desktop;` | 1, **done** |
| `app.rs` | 66 | `mod folderstorm_desktop;` | 1, **done** |
| `app.rs` (`draw_ui`) | 18160, after the gesture boards' line | one line: `self.folderstorm_desktop(&ctx, &p, &mut a);` (not at 17346 as first planned: section 19) | 1, **done** |
| `app/click_pick.rs` (`ui_takes_pointer`) | 295–296 | the position named, and one more term: `\|\| crate::folderstorm::desktop::takes_pointer(&self.egui_ctx, pos)` | 1, **done** |
| `settings.rs` | 526, 2051, 3322 | the field, its default, kept by the reset | 1, **done** |
| `ui/inventory.rs` (`InvAction`) | 152 | one variant, `FolderstormDesktopAdd(Vec<Uuid>)` | 2, **done** |
| `app/inventory.rs` | 302 | its arm, one line | 2, **done** |
| `ui/toolbar.rs` | 477, beside `dragging` | `pub(crate) fn carried_command(ctx) -> Option<String>` (5 lines: the payload is private) | 3, **done** |
| `ui/toolbar.rs` (`context`) | 406 | one line in a button's menu: `crate::folderstorm::desktop::toolbar_menu(ui, p, button.map(…));` | 3, **done** |
| `ui/widgets.rs` (`Floater::show`) | 596–603, after `toggle_min` is known | 8 lines, the shape of the `window_hidden` block above it: `let collapsed = minimized && !compact && !close && !toggle_min; if crate::folderstorm::desktop::taskbar::holds(ctx, &self, workspace::window_icon(self.id), collapsed) { if self.cascade.is_some() { cascade::hidden(…) } return None; }` | 4, **done** |
| `crates/aurora-i18n/locales/{en,fr,ja}.ftl` | the `fs-` block | `fs-desktop-*`, `fs-taskbar-*` (about 60 keys in all; 19 with checkpoint 1) | each |
| `docs/TESTING.md` | after the backgrounds' paragraph (438), and its row in the table | `AURORA_DEMO_DESKTOP` | 1 (**done**), then each |

No hook in: `ui/bars.rs` (we call `run_command`), `ui/toolbar/catalogue.rs`
(we add no command), `ui/layout.rs`, `ui/bars/islands.rs`,
`ui/inventory/context_catalog.rs`, `ui/inventory/drag.rs`,
`ui/options/pages/*.rs`, `keybinds.rs`, the renderer.

The only deep one is `Floater::show`. It changes nothing while the taskbar
is off (`holds` answers false first), it sits beside an upstream block of
the same shape, and it is the smallest way: the alternative (moving the
title bars into a row) would fight `ui/layout.rs`'s anchors and snapping.

## 10. Performance

- **Off, or no room: no work.** The frame call returns after one read of
  the settings and of `world_area` (as the background does).
- Rooms and cells are pure maths over a handful of rectangles, redone only
  when the window, the world, the monitors, the bars or the scale change
  (a signature kept in egui's memory).
- A shortcut is one icon of the Tabler atlas already loaded (no new
  texture, no decode) and one cached galley. 256 at most.
- A folder's name is one hash lookup per frame; its path is built only
  for the tooltip.
- The taskbar's list is rebuilt each frame from the floaters shown (tens
  at most), titles kept as `Arc<str>` and reused while unchanged. Nothing
  when the taskbar is off.
- No repaint asked, but during a drag or the rename field.
- A held window costs less than a collapsed one: nothing of it is laid out.

## 11. Accessibility and keyboard

- Every shortcut and taskbar button is a focusable widget with
  `WidgetInfo` (`Role::Button`, its name; "collapsed", "in front"): Tab
  reaches them, Enter or Space opens, the screen reader names them.
- On a focused shortcut: F2 renames, Delete removes, the menu key or
  Shift+F10 opens its menu, the arrows move the focus to the neighbour
  cell (checkpoint 3).
- Three keyboard actions in `folderstorm/keys.rs`, **none with a default
  key**: *Show or hide the desktop shortcuts*, *Focus the desktop*, *Focus
  the taskbar*.
- Names are never cut without a tooltip. The label's pill keeps the
  theme's contrast over any wallpaper. Greyed always comes with a reason
  in the tooltip. Sizes follow the interface scale.
- The drag is never the only way: every drop has a menu entry (*Add to
  desktop*, *Move to ›* a slot, *Arrange by name*).

## 12. Preferences and search words

Preferences › Layout › Bars and windows, a group *Desktop in the margins*
after *Background around the world*:

- *Shortcuts in the margins* (switch), *Icon size*, *Open with a single
  click*, the count and *Bring them here* when some are on a monitor not in
  use, *Remove all shortcuts…* (asks first).
- *Taskbar in a margin* (switch), *Monitor* (Automatic, 1, 2, 3…), *All
  open windows* (off: the collapsed ones only).
- A note while the world fills the window: "Shown when the window leaves
  room around the world (World viewport, above)."

Each row edits a tracked field, so the modified dots and the page's reset
work. Search words (`fs-search-*`, row 1.9): "desktop shortcut icon
launcher folder margin monitor", "taskbar minimized collapsed windows tray
dock".

## 13. Demo

`AURORA_DEMO_DESKTOP=<mode>` (listed in `docs/TESTING.md`), seeded from
`app/folderstorm_desktop.rs`. It turns the viewport on when it is off (the
world in the middle third), as the backgrounds' demo does.

```
AURORA_DEMO=1 AURORA_DEMO_DESKTOP=1 AURORA_WINDOW_SIZE=5760,1080 \
  AURORA_DEMO_MONITORS=1920x1080,1920x1080,1920x1080 \
  cargo run -p aurora-viewer -- --title "Test desktop"
```

| Mode | Shows |
|---|---|
| `1` | Left monitor: six commands and three folders of the demo inventory, one renamed with an icon of its own, one whose folder is gone. Right monitor: two shortcuts. A background from `AURORA_DEMO_BACKGROUND` may be added |
| `drop` | At frame 200 a demo folder is dropped from the Inventory on the left desktop; at 300 a shortcut is dragged to the right one |
| `open` | At frame 200 a folder shortcut is opened (its window beside it), at 300 a command (People) |
| `taskbar` | The taskbar on the right monitor: Inventory and a folder window collapsed (held), People open (not listed); at frame 300 the Inventory button is clicked |
| `overflow` | Twenty collapsed windows: narrow buttons and *More* |
| `rlva` | `@showinv` in force: folder shortcuts locked and unnamed, the Inventory command closes again |
| `single` | The window on one monitor: nothing shows, collapsed windows are title bars |

## 14. Tests

**Pure**
- `store`: the file read and written back unchanged, unknown entries
  kept, limits, add / move / swap / rename / remove / arrange, one desktop
  for each account, an earlier file's shortcuts given to their accounts.
- `place`: slots for one, two and three monitors, at 100 and 200 %, with
  negative desktop coordinates, without a monitor list; rooms for each
  place of the bars and with the world on part of a monitor; cells for
  each size; blocked cells; the nearest free cell is the same at every
  call; the taskbar's strip leaves the room.
- `targets`: the folder rule (own, Trash, Library, a link, gone); **every
  id of `COMMANDS` is classed**; the three gates; Favorites and VR view
  refused.
- `taskbar`: the click rule (restore, collapse, to the front), widths and
  overflow, the order, what is held.

**Frame tests (software painter, as `backgrounds/tests.rs`)**
- Shortcuts show in a margin **after the renderer's cut of the labels**,
  over the background, under a window. Nothing without a margin, nothing
  when off.
- `takes_pointer`: true over a room, false over the world, false under a
  window.
- A double click gives one request; a single click selects; with *single
  click*, one click gives it.
- A drag to another cell, a swap, a drag to another slot.
- An Inventory payload dropped adds folders, leaves items out, clears the
  payload. A toolbar payload dropped adds the command.
- Rename, the icon menu, remove.
- Taskbar: a collapsed floater is not drawn while held and its button is
  there; the click restores it where it was; off, or without a margin, it
  is Aurora's title bar; **a cascade window (a notecard) keeps its place
  after being held**; a compact-minimized window is never held; the
  attention mark; close from the menu closes it.
- Hidden interface, mouselook.

**App glue** (as the `Shell` tests of `app/folderstorm_workspaces.rs`): a
folder shortcut opens one window and then brings it forward; refused under
`@showinv`; a command reaches `run_command` and its action is handed over;
Sit refused while locked.

**Preferences**: the group, the switches, the count of unused shortcuts.

**Not checkable in the sandbox** (no GPU): the real frame, three real
monitors, mixed DPI, the feel of the drags. The human's test.

## 15. Checkpoints

Each is one PR from an up-to-date `main`, merged before the next starts.
Sizes are added lines, tests and translations included.

| # | What | Hooks | Size |
|---|---|---|---|
| 0 | This plan in `folderstorm/plans/`, row 1.10 in FEATURES.md | – (docs) | 1 file |
| 1 | **Command shortcuts.** The store, slots, rooms and cells; the desktops drawn and taking the pointer; *Add a command ›* from the desktop's menu; open (double click), move by drag with snapping, remove; the switch and icon size in Preferences; `AURORA_DEMO_DESKTOP=1` | `app.rs` (2), `click_pick.rs`, `settings.rs` | ~1,900 |
| 2 | **Folder shortcuts.** *Add to desktop* in the Inventory menu, the drop from Inventory, one window per folder placed beside the shortcut, gone and other-account folders, `@showinv`; *Show in Inventory* | `ui/inventory.rs`, `app/inventory.rs` | ~1,000 |
| 3 | **Editing and the toolbar** (built, section 22)**.** Rename, the icon list, *Arrange by name*, single-click switch, the unused-monitor note; drag and *Add to desktop* from a toolbar; keyboard (focus, arrows, F2, Delete) and the two desktop key actions | `ui/toolbar.rs` (2) | ~1,000 |
| 4 | **The taskbar** (built, section 23)**.** The windows seen, held windows, the island, clicks and menus, overflow, attention, its Preferences rows, its key action; demos `taskbar`, `overflow` | `ui/widgets.rs` | ~1,200 |
| 5 | **Item shortcuts** (asked by the human). In this order: landmarks (opening teleports, `InvAction::TeleportLandmark`), notecards (its window), outfits (through outfit safety, row 2.7), then wearables, whose shortcut **adds** the item to what is worn (*Add*, never *Replace*). Each kind through `world::inventory::actions` (`agent_mask`, as the gesture boards): an item id is kept, never an asset id. The record already keeps a target it cannot read (`kept`), so a build without them loses nothing | to be planned (the Inventory's menu entry is ours already) | to be planned |
| 6 | **Later, each on request.** A layout's own desktop and taskbar choices; *Keep the taskbar above the windows*; several selected at once; lit while open; a colour of its own for a shortcut; the menu key on a focused shortcut; MCP tools | to be planned | – |

Checkpoint 4 needs only checkpoint 1's `place` and settings field. If the
taskbar matters more to the human than the shortcuts, it can go second.

## 16. Risks

1. **`Floater::show` is upstream's busiest function.** Our five lines may
   conflict at a sync. They are marked, sit beside a block of the same
   shape, and a frame test fails if they are lost.
2. **Cascade groups** (verified, section 3): without `cascade::hidden` a
   held notecard, script or torn-off conversation is placed again as new
   when restored. In the hook, tested.
3. **One frame of title bar.** A held window that is closed or expanded
   by something else (its toolbar button, Ctrl+Shift+W) is drawn collapsed
   for one frame before it goes or grows. If it shows, the hook also takes
   `open` and does the close itself.
4. **`is_pointer_over_egui()` is read directly in about ten places** of
   `app.rs` (hover tips `:12021`, the camera tool's cursor `:4366`, the
   pipettes `:9901`, the beacon `:12086`…), not through `ui_takes_pointer`.
   Over a desktop they still see "the world". A pick there hits nothing,
   so nothing happens, but the cursor can show the Alt-camera shape over a
   shortcut. Fix only if seen (one more hook, in `pointer_over_ui`).
5. **The order of `Order::Background` areas** against egui's background
   layer is egui's behaviour of today (0.37, read in its source), not a
   promise. The frame test guards an egui upgrade.
6. **Bars in the margins** (*Around the world*, *Over the window*): the
   room's maths must match where the bars really are, or shortcuts hide
   under an island. Tested for the three places; to check by eye.
7. **Slots are counted left to right.** A monitor added on the left moves
   every desktop one monitor to the right. Nothing is lost; *Bring them
   here* and the drag repair it.
8. **Wayland**: no monitor list, so one desktop over all the margins, and
   a shortcut can straddle a bezel.
9. **Icons are rasterised at 64 pixels** (`ui/icons.rs:771`): the Large
   size (48 points) is soft at 200 %.
10. **Three monitors were never tried** (row 1.1 says so), and this
    feature is made for them.
11. **The taskbar under a window** (decision above) may surprise someone
    used to Windows.
12. **The command table drifts**: an upstream command with a new gate in
    the toolbar's row would run ungated from a shortcut. The table's test
    makes each sync look at new commands; the gates are re-read then.
13. **`@showinv` and folder windows already open**: Aurora closes the
    main, additional and tab windows (`ui/inventory.rs:786`), not the
    folder windows. Not ours to change here; noted for the human.

## 17. Open questions

1. **Taskbar content**: only the collapsed windows (the default here,
   chosen by the main session as the closest to the request's words), or
   every open window (the switch *All open windows*)?
2. **Taskbar above the windows**: like Windows (always), or like Aurora's
   dock (the default here)?
3. **No margin**: is "no taskbar, title bars as before" right, or should
   the taskbar then sit at the bottom of the world?
4. **Double click or single click** by default?
5. **Items**: are landmark, notecard or outfit shortcuts wanted, and which
   first?
6. **Layouts**: should a layout or a workspace be able to carry its own
   set of shortcuts, or only hide them and place the taskbar (the "later"
   here)?
7. **Per profile, folders per account** (as the layouts): right, or one
   desktop per account (as the gesture boards)? **Answered: one desktop
   per account** (section 21).
8. **`@showinv`**: hide the folder shortcuts' names (the default here), or
   only refuse to open?
9. **A folder's default action**: its own window (the default here), or
   the main Inventory on that folder?
10. **The icon list**: about 40 chosen icons, or every Tabler icon with a
    search?
11. **Off by default**: right for both switches?

## 18. Verified in code vs inferred

**Verified** (read in the sources named in section 3): the labels' cut and
the marker; the background's mark and splice; egui's layer order and
`is_pointer_over_egui`; `ui_takes_pointer` and what a press in a margin
does; the catalogue, `run_command`, the row's gates and pop-ups; the drag
payloads and their visibility; `open_folder_window` and the workspace's
`@showinv` refusal; `Floater::show`'s minimized state, its helpers, the
hidden-window early return, the cascade registry's two-frame memory; the
islands; the layouts' capture; the settings' reset; that no row of
minimized windows exists; the C++ docs listed in section 2.

**Inferred, to check while building**: that every `BarAction` a command
returns is checked by the app as for a toolbar click (read for Sit, Home
and Build only); the exact room when the bars are *Around the world*; that
a text field edits well in an `Order::Background` area; that 256 shortcuts
cost nothing measurable; the look of a label's pill over a bright
wallpaper; how winit orders monitors on the human's PC (we sort, so it
should not matter).

**Not done**: nothing was built or run. No cargo command was needed.


## 19. Checkpoint 1 as built

What the build changed against the sections above, and what it found.

- **The frame call is after the windows' calls, not after the bars**
  (`app.rs`, beside the gesture boards' line). At the place first planned
  the skin's icons are still borrowed by `draw_ui`, so a method of the app
  cannot be called there. It changes nothing on screen: a desktop is an
  area of egui's background order, painted under every window wherever it
  is called from, and egui tests the pointer against the last frame's
  rectangles.
- **Speak is a third button-only command**, with Favorites and VR view:
  its button is a key held, and `run_command` does nothing for it. The
  three are listed greyed in *Add a command*, with the reason.
- **A fourth gate**: the two-layout toggle (`layout_toggle`, ours) is
  greyed while no layout is set to toggle to, as its toolbar button is
  (`workspaces::toolbar::look`).
- **A room is one rectangle**: the largest part of the slot that is clear
  of the world and of the bars, 8 points in from its edges. Bars *Inside*
  or *Around the world*: the slot less the bars' area
  (`viewport::bars::side_screen`). Bars *Over the window*: the slot within
  Aurora's work area, less the world. So "a cell under a bar" never
  exists; a blocked cell is one outside the grid (the window got smaller,
  or the world moved), and the shortcut is shown in the nearest free cell.
- **The record's taskbar field** is `all_windows` (absent: the collapsed
  windows only), after the main session's change of the default.
- **In the record but without a control yet**: `single_click` (honoured by
  the surface, its switch comes with checkpoint 3), `name` and `icon`
  (shown when set; the demo sets them; edited with checkpoint 3), a folder
  target (kept, shown greyed "Not available yet" to its account until
  checkpoint 2), `taskbar`.
- **The desktop's menu** has *Add a command ›*, *Icon size ›* and *Desktop
  settings…*. *Toolbar buttons…* and *Arrange by name* come with
  checkpoint 3.
- **Keyboard, so far**: a shortcut is a focusable button (Tab reaches it,
  Enter or Space opens it, Delete removes it, the screen reader names
  it). A click did not take the keyboard focus, as for Aurora's buttons;
  since the human's decision of 2026-11-17 it does (see checkpoint 3's
  decision on the keyboard).
- **Not done in checkpoint 1**: a notice when a room has no free cell for
  *Add a command* (the command is then not added; done since, see "A full
  desktop" in section 20); the count of shortcuts on a monitor not in use;
  *Remove all shortcuts…*.
- **Tests**: `folderstorm/desktop/{store,place,targets}.rs` (pure),
  `folderstorm/desktop/tests.rs` (frames, with the renderer's cut of the
  world's labels), `app/folderstorm_desktop.rs` (a command reaches
  `run_command`, the gates).

## 20. Checkpoint 2 as built

Folder shortcuts, on `claude/margin-desktop-folders`. What the build
changed against the sections above, and what it decided.

- *(Replaced by section 21: each account has its own desktop, and another
  account's shortcuts are not shown at all.)* **A folder of another
  account is shown greyed, not hidden** ("This
  folder is in another account's inventory", *Remove* in its menu), by
  the main session's brief for this checkpoint; sections 5 and 6 said "not
  shown". Nothing of the folder is read for it: it wears the generic name
  "Inventory folder" (or the name the user typed). `Desktop::visible` is
  gone. To hide them again: one filter in `surface::show`.
- **Open question 8 as the plan's default**: under `@showinv` a folder
  shortcut wears a lock and "Inventory folder"; its folder's name, its
  path, its colour and even the name the user gave the shortcut are
  hidden (that name is often the folder's). It does not open, *Show in
  inventory* is greyed, a drop is refused, and the app's arm of *Add to
  desktop* refuses too (the Inventory is closed then anyway).
- **Open question 9 as the plan's default**: a folder shortcut opens the
  folder's own window. One window per folder is the glue's rule
  (`app/folderstorm_desktop.rs` `open_folder`): an open window of that
  folder is brought to the front (expanded when collapsed) instead of a
  second one. A folder window has a new id each time it is opened, so
  every new one is placed beside its shortcut (`place::beside`: to its
  right, else to its left, kept in the room), at Aurora's size for such a
  window (420 × 440).
- **The folder rule** is `targets::folder_state`: live (under the
  agent's root, out of the Trash), gone (not in the inventory, in the
  Trash, or the Trash itself), refused (the root, the Library and what it
  holds), unknown (the inventory's root is not known yet: greyed "Not
  available yet", never "gone"). A link to a folder gives the folder.
- **One reason for every refusal to add**: "Only folders of your own
  inventory, out of the Trash, can be desktop shortcuts." (an item, the
  Library, the Trash, the root).
- ***Add to desktop* needs no new menu command**: the entry is added by
  our `context_catalog/folder_tabs.rs` (called for folders and items in
  both Inventory styles) as `Command::Invoke(InvAction::…)`. Its entry id
  for the menu layouts is `desktop-add`. On a selection with folders and
  items it is offered, and the items are counted in the notice.
- **From the menu**: the folders go to the first free cells of the first
  room; the switch is turned on when it was off and the notice says so;
  without a margin they are kept (slot 0, columns of 8) and the notice
  says when they show. The settings are saved at once.
- **A drop** lands in the cell under the pointer; a taken cell, and the
  other folders of a selection, take the nearest free cells. A drop that
  worked says nothing (it is seen); what was left out, already there or
  without room is said in a notice (the chat's notices, topic Items). The
  payload is cleared in every case, so nothing reaches the world's own
  drop.
- **Notices, not a toast in the margin**: the desktop has no place of its
  own for a message, and Aurora's notices are where the Inventory's own
  refusals go.
- **A folder's colour** is read from the Inventory's preferences
  (`folder_colors`, when they are on) without a hook; a greyed shortcut
  is never tinted.
- **The tooltip of a folder shortcut** is its path in the Inventory,
  built only while the tooltip shows. No tooltip while Inventory is
  dragged over the desktop.
- **Demo**: `1` now has four folder shortcuts (three of the demo
  inventory, one gone); `open` opens a folder at frame 200 and People at
  300; `drop` adds a folder as a drop would at frame 200 (the pointer
  cannot be played) and moves it to the third monitor at 300; `rlva`.
  The folders are those of the demo's login (Clothes, Landmarks, the
  Photo Album, Objects).
- **A full desktop says so** (the human's decision, on
  `claude/margin-desktop-full-notice`). Every way of adding to a room
  with no free cell (*Add a command ›*, *Add to desktop* from the
  Inventory, a drop) adds nothing there and says, in the chat's notices
  (topic Items): "This desktop is full: remove a shortcut or use a smaller
  icon size." When some folders of a selection went in, the notice starts
  with how many did not fit. Only the target room counts: nothing flows to
  another monitor's desktop (section 5 does not say it should). At the
  profile's limit (256 shortcuts) the notice says that limit instead.
  While the room takes no new shortcut, *Add a command ›* is greyed with
  the same reason as its tooltip (the desktop's menu is then reached on
  the room's edge past its cells). A shortcut that desktop has already is
  still found and selected, full or not (`surface::add_command`,
  `add_folders`). `surface::NoRoom` holds the two reasons.
- **Not done in checkpoint 2**: a drop is not offered on a slot without a
  room (there is nothing to drop on); *Move to ›* a slot from the menu
  (checkpoint 3, with the keyboard); folder windows already open when
  `@showinv` starts are left to Aurora (risk 13).
- **Tests**: `targets.rs` (the folder rule, what a selection gives, a
  folder's face under each state), `place.rs` (`beside`), `store.rs` (a
  folder kept with its account), `tests.rs` (frames: a folder shortcut's
  name, its request and its menu; gone and other-account folders;
  `@showinv`; drops of a folder, a mixed selection, the same folder, an
  item, the Library, over the world; adding without a room and in a full
  one; a full room and the profile's limit on every way of adding, the
  greyed *Add a command ›*), `app/folderstorm_desktop.rs` (one window per folder, refused
  under `@showinv`, gone folders),
  `ui/inventory/context_catalog/folder_tabs.rs` (the menu entry).

## 21. One desktop per account, as built

The human's decision, on `claude/margin-desktop-per-account`: "**Each
account should have its own desktop.**" It answers open question 7 and
replaces what sections 5, 6, 19 and 20 said of accounts.

- **Per account**: the shortcuts (commands and folders), their slot and
  cell, their own names and icons. Two accounts on one profile see two
  desktops; switching back restores the first. Nothing of another account
  is shown: the greyed "another account" folder shortcut, its tooltip
  (`fs-desktop-greyed-other-account`) and `Greyed::OtherAccount` are gone.
- **Per profile**: the switch, the icon size, `single_click`, the
  taskbar's choices (the reasons are in section 6). Preferences is
  unchanged but for a sentence in the switch's help.
- **The limit** (256 shortcuts) is now each account's.
- **The record** (section 6): `accounts`, account id → `{ next_id,
  shortcuts, kept }`. A folder target is `{ folder }`: its account is the
  key. In memory, `store::Desktop` holds the shortcuts of the account
  logged in (`shortcuts`, as before, so the surface is unchanged) and the
  others aside (`others`); `Desktop::sign_in(account)` swaps them, and the
  app calls it each frame and before *Add to desktop*
  (`app/folderstorm_desktop.rs` `desktop_sign_in`; a comparison when the
  account is the same). What is written does not depend on who is logged
  in.
- **Migration** of a file of checkpoints 1 and 2, when it is read: a
  folder shortcut goes to the account it named; the commands, a folder
  without a readable account and the entries that could not be read wait,
  written back in their old place, for the first account that logs in.
  That `sign_in` returns true and the app saves at once, so that a second
  account cannot take them. A shortcut keeps its id unless the account has
  that id already (it then takes the next one); beyond the account's limit
  it is kept unread (`kept`), not dropped. Two shortcuts could then name one cell,
  but only in a hand-edited file holding both shapes: nothing special is
  done for it.
- **Lenient reading**: an entry of `accounts` whose key is not an account
  id, or that is not a table, is kept and written back; an account's
  shortcuts are read as the profile's were (unknown targets kept, values
  brought back into range, an id met twice kept unread).
- **Going back to a build of checkpoints 1 to 3** would not show the
  shortcuts (it does not read `accounts`) and would drop them at its next
  save. Those builds were never merged.
- **No upstream hook** added or moved.
- **Demo**: unchanged for the eye; the demo's desktop is made for the
  demo's account.
- **Tests**: `store.rs` (each account has its own desktop and switching
  back restores it; a file of the earlier shape goes to its accounts,
  whichever logs in first, before and after a save; lenient reading of
  `accounts`, ids met twice, an account at its limit), `tests.rs`
  (frames: a second account sees none of the first's shortcuts and
  cannot open them from where they stood; back to the first; logged out),
  `targets.rs` (the other-account face removed).

## 22. Checkpoint 3 as built

Editing, the toolbar and the keyboard, on `claude/margin-desktop-cp3`
(over the per-account desktop of section 21). What the build changed
against the sections above, and what it decided.

- **Rename** (*Rename* in a shortcut's menu, F2 on the focused one): the
  name is edited in a field under the icon, its text selected. Enter, or a
  click elsewhere, keeps it; Esc leaves it as it was. An empty name, **or
  the target's own name typed back**, gives the shortcut no name of its own
  (it then follows its folder's name again). The name is cleaned and cut at
  64 characters (`store::clean_name`); the folder is never renamed. After
  Enter or Esc the keyboard stays on the shortcut.
- **Under `@showinv`** a folder shortcut's *Rename* and *Icon ›* are greyed
  ("Blocked by RLVa") and F2 does nothing: its name and icon are hidden
  then, and nothing is typed over them. *Arrange by name* orders by the
  names shown, so the hidden folders sort among themselves by their age.
- **The icon list** is 40 Tabler icons **the viewer already embeds**
  (`desktop/icons.rs`): `ui/icons.rs` lists its icons one by one, so a new
  one would be one more upstream hook each. *Icon ›* is *Default* (ticked
  while the shortcut has none of its own) over a grid of 8 × 5; the one in
  use is outlined. The icons have no text of their own (40 names in three
  languages for a picture): the screen reader hears their Tabler name.
- ***Arrange by name*** (the desktop's menu) puts that desktop's shortcuts
  from its first cell, down then right, by the name shown (case ignored),
  then by age. Greyed with fewer than two shortcuts.
- ***Move to ›*** lists the monitors that show a desktop now, but the
  shortcut's own ("Monitor 3"); absent with one desktop. The shortcut goes
  to that desktop's first free cell; refused with a notice when that
  desktop has a shortcut to the same thing (it is selected) or is full.
- ***Toolbar buttons…*** in the desktop's menu opens Aurora's window
  (`ToolbarUi::open`), with Aurora's own label (`toolbar-edit`).
- **A toolbar button dropped on a desktop** (from a toolbar, or from the
  *Toolbar buttons* window; toolbars unlocked, as today) adds a shortcut in
  the cell under the pointer: a copy, the button stays. The desktop is
  outlined and the cursor says copy; for Favorites, VR view, Speak and a
  command Aurora cannot run yet, the outline is red and the drop says why
  in the chat's notices. The payload is cleared, so no toolbar takes it.
- ***Add to desktop* in a toolbar button's menu** (after Aurora's own
  entries for that button, before the alignment choices): greyed with the
  reason for the same commands. It asks (`desktop::ask`); the app's glue
  adds the shortcut at its next frame, in the first free cell of the first
  desktop, turns the shortcuts on when they were off, and says what it did
  in the chat's notices, as *Add to desktop* of the Inventory does. Without
  a margin the shortcut is kept for when there is one.
- **A command that never opens from a shortcut is no longer added** by any
  way (`surface::add_command`, `CommandAdded::Never`); one that has nothing
  to do now (an empty Photo roll) still is.
- **A click gives the keyboard to the shortcut** (the human, 2026-11-17:
  "F2 and Delete working on a selected icon would be cool"): F2, Delete,
  Enter / Space and the arrows work on the clicked shortcut at once, as
  well as on one reached with Tab or the key action *Focus the desktop*.
  A press anywhere else (the world, a window, the room) gives the
  keyboard back, as egui gives up a focused widget's focus on a press off
  it, so the avatar's keys work again after a click in the world. One
  line in `surface::show` (`request_focus` on a primary click). Checkpoint
  1's first rule (a click kept the keyboard on the world) is replaced.
- **The arrows** go to the nearest shortcut on that side of the same
  desktop, one of the same row or column being preferred
  (`place::neighbour`); at the edge they stay. They do not cross to another
  monitor's desktop. egui's own arrow navigation is turned off on a focused
  shortcut (`set_focus_lock_filter`).
- **Two keyboard actions** in `folderstorm/keys.rs`, without a default
  key: *Show or hide the desktop shortcuts* (the switch, saved) and *Focus
  the desktop* (the selected shortcut, else the first of the first
  desktop). Their section of Preferences › Shortcuts is now "Layouts and
  desktop". *Focus the taskbar* comes with checkpoint 4. **Not done**: the
  menu key / Shift+F10 on a focused shortcut (Aurora's menus open on the
  pointer only).
- **Preferences** (*Desktop in the margins*): **behind the cog of the
  switch**, *Open with a single click* (the profile's, off by default) and
  *Remove all shortcuts…*, which asks in its row before removing (this
  account's only; what this version cannot read is kept); under the icon
  size, "N shortcuts are on a monitor that is not in use" with *Bring them
  here*, shown only while it is so. The cog keeps the page as long as it
  was: two more rows above Aurora's own pushed a row of its page out of
  the frame of one of upstream's tests
  (`keyboard_scale_survives_an_unrelated_preference_change_and_reload`),
  which is not ours to edit.
- **"On a monitor not in use"** counts the shortcuts of slots without a
  room while another slot has one. While no desktop shows at all (the
  world fills the window) nothing is counted: the group's first note says
  why nothing shows. ***Bring them here*** moves them to the first free
  cells of the first desktop; **one whose target that desktop has already
  is removed** (a desktop never holds the same thing twice), and those a
  full desktop cannot take stay where they were.
- **Upstream hooks**: the two of `ui/toolbar.rs` (section 9). The second
  passes the button's command id rather than the button.
- **Demo**: `edit` (the name field at frame 200, a name and an icon given
  at 300) and `unused` (two shortcuts on a monitor not in use, Preferences
  on the group at frame 200).
- **Tests**: `store.rs` (rename, icon, arrange, unused, clear for one
  account), `place.rs` (`neighbour`), `icons.rs` (40, embedded, once),
  `tests.rs` (frames: rename from the menu and with F2, Enter, Esc, the
  own name typed back, the limit; nothing edited under `@showinv`; the
  icon menu and *Default*; *Arrange by name*; *Move to ›* and its refusal;
  *Toolbar buttons…*; a toolbar button dropped, twice, refused, over the
  world; the first free cell from a button's menu, without a room, the
  notices; the hook's menu entry and its ask; the key action, the arrows,
  Enter, Delete, and no key without the keyboard; the count and *Bring
  them here*, a full desktop, no desktop at all; the demo's modes),
  `folderstorm_search.rs` (the group's rows found), `keys.rs` and
  `workspaces/tests/switcher.rs` (the two actions, without a key). The
  Preferences rows themselves are not drawn in a test.
- **Left for checkpoint 4**: the taskbar, as planned (the `Floater::show`
  hook, `desktop/taskbar.rs`, its Preferences rows, *Focus the taskbar*,
  demos `taskbar` and `overflow`). **Checkpoint 5**: item shortcuts
  (section 15).

## 23. Checkpoint 4 as built

The taskbar, on `claude/margin-desktop-taskbar` (over checkpoint 3). What
the build changed against the sections above, and what it decided.

- **The island** (`desktop/taskbar.rs`, `show`): one of Aurora's islands
  (`ui::bars::islands::area`, 36 points high) along the bottom of one
  room, on the windows' layer. A window dragged over it covers it and a
  click on it brings it back; the windows keep their title bar clear of
  it, as of the dock (`islands::area` tells `layout::island_drawn`). With
  nothing to list it says "No collapsed windows" (or "No open windows").
- **Where**: *Automatic* is the widest room, the first of two as wide
  (`place::taskbar_slot`). A chosen monitor without a room gives **no
  taskbar**, not another monitor's. The strip is taken out of that
  monitor's desktop (`place::taskbar_strip`, `desktop::rooms`): its last
  rows of cells go, and a shortcut stored there is shown in the nearest
  free cell, its stored cell kept (the rule of a blocked cell).
- **The hook** in `Floater::show` (`ui/widgets.rs:596–603`, 8 lines, the
  only upstream change of the checkpoint besides the translations): every
  open floater tells `taskbar::holds` its id, title, icon, attention and
  whether it is collapsed; held, it returns before anything is laid out,
  after `cascade::hidden` for a window of a cascade group (risk 2). With
  the taskbar off, `holds` is one read of egui's memory and false.
- **Held** means collapsed, the taskbar standing, a window of the normal
  layer (`Order::Middle`), not closing, not expanding, and without a
  compact body when minimized (Performance). Its place, size and collapsed
  state stay where Aurora keeps them: restored, it is where it was; the
  taskbar turned off, or the margin gone, it is Aurora's title bar where
  it stood. The settings keep nothing but the three choices
  (`store::Taskbar`: `on`, `slot`, `all_windows`).
- **The windows are read as the hook saw them**: those drawn before the
  taskbar in the frame (most: the call is at `app.rs:18160`), and those of
  the last frame for the ones drawn after it. A window not heard of for a
  frame is forgotten.
- **The order**: the collapsed ones in the order they were collapsed;
  with *All open windows*, every floater in the order it was first seen.
  A window closed, or hidden with the interface, and back takes a new
  place at the end.
- **A button**: the window's icon (`workspace::window_icon`, or the
  header's own) and its title, cut with "…", the whole title in the
  tooltip; `Role::Button`, spoken "…, collapsed" or "…, in front"; the
  attention mark of `widgets::attention_mark` with the window's own
  flash. With *All open windows* the front window is lit and a collapsed
  one is muted.
- **Clicks** (`taskbar::click`): collapsed → restored and brought forward
  (`widgets::bring_floater_front`); with *All open windows*, the one in
  front → collapsed (`minimize_floater`), another → to the front. Middle
  click closes (`close_floater`: the window's own questions run). Right
  click: *Restore*, or *Bring to front* (greyed for the one in front) and
  *Collapse*; then *Close*. Enter or Space on a focused button is a click.
- **Too many** (`taskbar::fit`): the buttons share the room, 180 points
  wide at most; under 96 points each they are their icon alone (32
  points); those that still do not fit go in *More* at the right end,
  with their count, a menu of their icons and titles where a click does
  what the button would.
- **The key action** *Focus the taskbar* (`folderstorm/keys.rs`,
  `Key::DesktopTaskbar`, no default key) gives the keyboard to the first
  button; Tab goes on from there.
- **Preferences** (*Desktop in the margins*, after the shortcuts' rows):
  *Taskbar in a margin* and, behind its cog, *Monitor* (*Automatic* and
  the numbers of the monitors that have a room now, plus the chosen one
  even when it has none) and *All open windows*. Three tracked fields;
  search words `fs-search-taskbar`.
- **Hidden with the interface** (its shortcut, immersive mode, mouselook
  without *Show the interface in mouselook*): no island, the windows stay
  held. In mouselook with the viewport dropped (no margin) they stay held
  too, and come back as buttons after it.

Decided in the build, beyond the plan:

- **A held window that was on top hands the front over** to the next
  window that is drawn (`hand_the_front_over`). Aurora's Ctrl+W closes the
  topmost window, collapsed or not: without this it would close a window
  that is not on the screen.
- **"In front" is the topmost floater that is not held**, read from
  egui's layers, not `widgets::floater_is_front`: a click on the taskbar
  puts the taskbar's own island on top.
- **A window of another layer is never held** (the poser's, over the
  photo viewfinder): it is a tool of what is under it. It is listed with
  *All open windows*.
- **The frame of title bar of risk 3 is taken off the screen** for the
  windows drawn before the taskbar: a held window being restored or
  closed is drawn collapsed once by `Floater::show`, and the taskbar moves
  that frame's shapes of its layer away. **Left**: a window drawn after
  the taskbar in the frame (those of `app.rs` after line 18160) may show
  its title bar for one frame on its way out. To look at on a real
  screen; the plan's remedy (the hook taking `open`) stays possible.
- **`desktop::rooms` takes the desktop, not its size** (it needs the
  taskbar's choices): its callers in `app/folderstorm_desktop.rs` and the
  tests follow.
- **No hook in `ui_takes_pointer`**: an island already counts as
  interface, and the island senses clicks between its buttons.
- **Not done**: *Keep the taskbar above the windows* and a layout's own
  taskbar choice (checkpoint 6, as planned). The Preferences rows are not
  drawn in a test (their words are searched, their fields are tracked).

**Upstream hooks**: `ui/widgets.rs:596–603` (above), and the keys of
`crates/aurora-i18n/locales/{en,fr,ja}.ftl` (15 each: 13 `fs-taskbar-*`,
`fs-keys-desktop-taskbar`, `fs-search-taskbar`). Nothing in `app.rs`: the
taskbar is drawn from checkpoint 1's line (`app.rs:18160`, through
`app/folderstorm_desktop.rs:106`), and its key action goes through the
arm of `Action::Folderstorm` that the layouts added (`app.rs:5012`).

**Demo**: `taskbar` (the taskbar on the third monitor; at frame 200 the
Inventory and a folder's window are opened and collapsed, People is
opened and not listed; at frame 300 the Inventory is restored as a click
on its button does) and `overflow` (twenty folder windows collapsed at
frame 200: icon buttons and, when the monitor is too narrow for twenty,
*More*). Both in `docs/TESTING.md`.

**Tests**: `taskbar.rs` (the click rule, the widths and *More*, the two
orders), `place.rs` (the slot, the strip), `taskbar_tests.rs` (frames,
with real `Floater` windows, one drawn after the taskbar: held and
restored where it was at its size; off, or without a margin, title bars
again; a cascade window keeps its place; a compact one is never held; the
menu, the middle click; *All open windows*; the front handed over; narrow
buttons and *More*; hidden with the interface and in mouselook; the key
action and Enter; the attention; the strip out of the desktop's room; the
demo's seed; the icons embedded), `folderstorm_search.rs` (the rows and
the key action found in three languages), `keys.rs` and
`workspaces/tests/switcher.rs` (the third action, without a key). The
frame tests fail if the hook is lost at an upstream merge.

**Left for checkpoint 5**: item shortcuts (section 15).

## 24. Checkpoint 5 as built

Item shortcuts, on `claude/margin-desktop-items` (over checkpoint 4 and
the click that gives a shortcut the keyboard). Asked by the human, in this
order: landmarks, notecards, outfits, then wearables that are **added**.
What is built is listed kind by kind; each kind is its own commit.

### What every item shortcut shares

- **The record**: a third target, `{ item: <uuid> }` (`store::Target::Item`):
  the **inventory item id**, under the account (section 21). Never an asset
  id, a name or the item's kind: the kind is read from the inventory when
  the shortcut is drawn (`targets::ItemKind::of`, by asset type). A build
  that does not know a kind shows the shortcut greyed ("This kind of item
  cannot be opened from the desktop") and keeps it.
- **Links are followed when the shortcut is added** (`actions::original`,
  as LLInvFVBridgeAction::doAction): a link and its item give the same
  shortcut, which keeps the item's id. A stored link is followed again
  when it is drawn.
- **Adding** is checkpoint 2's, extended: *Add to desktop* in the
  Inventory's menu (the same entry, `desktop-add`, the same
  `InvAction::FolderstormDesktopAdd`: it now carries items too) and a drop
  on a desktop. A selection gives its folders and its items that can be
  shortcuts; the rest is counted in the notice ("N entries left out").
  The full-desktop and the account-limit notices are checkpoint 2's
  (`surface::add_targets`, which `add_folders` now calls).
- **What it shows**: the item's name, read now, under the Inventory's own
  icon for its type (`ui::inventory::item_icon`, the skin's `Inv_*`); its
  path in the Inventory as tooltip; *Rename*, *Icon ›*, *Move to ›*,
  *Remove* as any shortcut. Its menu's first entry says what opening does
  (*Teleport* for a landmark, *Open* for a notecard), then *Show in inventory* (the main
  Inventory window on the item, `InvAction::ShowMain`).
- **Greyed, with the reason**: not in the inventory ("deleted, or its
  folder is not loaded yet": the inventory loads folder by folder, and
  the two cannot be told apart, as for a gesture board's tile), in the
  Trash, of a kind a shortcut does not open, the inventory not known yet,
  refused by RLVa. A greyed item shortcut says nothing of its item (the
  generic "Inventory item", or the name the user typed).
- **`@showinv`** (stricter than the Legacy Viewer, as decided for
  folders): every item shortcut wears the lock and "Inventory item", its
  name, its path and the name the user typed are hidden, it does not
  open, it is neither renamed nor shown in the Inventory, and nothing is
  added.
- **Creators' permissions**: no new rule. A shortcut opens an item only
  through the path the Inventory uses for that kind, so the rights are
  checked where they already are; nothing of an asset is read, copied or
  shown by the desktop, and no asset id is kept or drawn (tests look for
  it in what is written and in what is painted).
- **Upstream hooks**: none added. The menu entry and its action are
  checkpoint 2's (`ui/inventory.rs:153`, `app/inventory.rs:302`), and the
  drop reads the Inventory's own payload.

### Landmarks

- **Opening teleports, after Aurora's question.** The glue calls
  `PlacesUi::confirm_landmark` (the entry Aurora made for "a landmark
  opened from elsewhere", the Legacy Viewer's TeleportFromLandmark that
  LLLandmarkBridgeAction::doIt shows): "Do you really want to teleport
  to…", with its "Do not show again" box (`Settings::confirm_teleports`).
  Yes goes through `PlacesAction::TeleportLandmark` to
  `request_user_teleport`, the one path of every teleport the user asks
  for. **Stricter than Aurora's Inventory**, whose double click teleports
  at once: on a desktop a stray double click must not.
- **RLVa**: the shortcut is greyed "Blocked by RLVa" while
  `World::rlva_can_teleport_landmark` is false (what greys *Teleport* on
  a landmark of the Inventory: `@tplm`, `@tploc`, `@unsit` while
  seated…), and `request_user_teleport` checks the command again when
  the question is answered.
- **The asset id** of the landmark names it in the teleport request
  (`NetCommand::TeleportLandmark`, as `InvAction::TeleportLandmark`
  carries it). It is read from the inventory when the shortcut is opened,
  handed to that path, and never kept, logged by us or shown.
- **The Library**: a landmark of the Library can be a shortcut (the
  Legacy Viewer and Aurora's Inventory teleport to it). **Decision**:
  what only reads an item may come from the Library; folders of the
  Library stay refused (checkpoint 2).
- **No right is asked** for a teleport (the Legacy Viewer asks for none):
  a no-copy, no-modify, no-transfer landmark teleports as a full one.

### Notecards

- **Opening is the Inventory's own *Open***: the glue hands the item to
  `apply_inventory_action(InvAction::Preview(item))`, the arm a double
  click on a notecard of the Inventory reaches. That arm, not the
  desktop, checks RLVa's `@viewnote` (`Inventory::can_preview`), opens
  Aurora's notecard window (one per notecard: opened again, it comes to
  the front with its unsaved text), checks the right to read
  (LLPreviewNotecard::loadAsset: copy, or a god; otherwise the window
  says "You are not allowed to read this content") and only then asks
  the grid for the content. Saving goes through the same window, with
  its own check (`preview::can_edit`).
- **Nothing of the notecard is read by the desktop**: not its asset id,
  not its text, not its rights. A notecard the agent may not read is
  still a shortcut (the Inventory shows it too), and its window refuses.
- **RLVa**: greyed "Blocked by RLVa" under `@viewnote`; the name stays.
- **The Library**: allowed, read through the same rights (the rule of
  section "Landmarks").
- The notecard's window is Aurora's own floater, so it is placed by
  Aurora's cascade, not beside the shortcut.
