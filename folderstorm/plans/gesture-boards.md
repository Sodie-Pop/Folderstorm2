# Gesture boards

FEATURES row 3.1. A window of buttons ("tiles") that play gestures and
sounds of the user's inventory, in several saved boards. Branch
`claude/gesture-boards`.

Sources, read as information: Folderstorm (C++)'s `doc/plans/gesture-board.md`
(checkpoints 1 to 3, the "compact board refinement", the inventory-sound
follow-up and the "defaults, bulk editing and larger boards" section) and
`doc/help.md` "Gesture boards"; Aurora's Gestures window
(`ui/gestures.rs`, `app/gestures.rs`) and its inventory sound actions
(`ui/inventory/context.rs` `double_click`, `app/inventory.rs`
`InvAction::TriggerSound`).

## The human's spec

- A board of buttons that play gestures, and sounds (sound inventory items).
- Several saved boards.
- Resizable buttons: all at once (the board's size) or one by one. By
  default every button has the same size, a long name is cut short. A
  toggle "Show full names" widens a button to its full name, never wider
  than the board.
- A button can be renamed (its own label, the inventory item keeps its
  name) and coloured.
- A sound's button shows it: a small speaker in its corner.
- Pages.
- The window is resizable.
- Ctrl+F searches all boards, or the current board only when toggled.

## Where the code goes

All of it in fork files:

| File | What |
|---|---|
| `crates/aurora-viewer/src/folderstorm/gesture_boards/mod.rs` | the module, the window id, its open state (egui memory, as the layouts' switcher), the toolbar command |
| `.../gesture_boards/store.rs` | the data model as `settings.json` keeps it, limits, robust reading, operations (add, move, remove, duplicate, rename…). Pure |
| `.../gesture_boards/layout.rs` | button sizes, the "full names" width, shelf packing into pages, search. Pure |
| `.../gesture_boards/folders.rs` | whole folders dropped on a board: the walk, its order, what it leaves out, the summary. Pure |
| `.../gesture_boards/window.rs` | the window (egui): top strip, tiles, pages, search, menus |
| `.../gesture_boards/dialogs.rs` | the button editor, the Add picker, the board name and delete dialogs |
| `.../gesture_boards/tests.rs`, `tests/` | pure tests and frame tests (with pictures) |
| `crates/aurora-viewer/src/app/folderstorm_gesture_boards.rs` | the app's side: draws the window with the inventory and the gesture manager, plays what is clicked through Aurora's own paths, saves the settings, fetches missing items, the demo |

Upstream hooks (each one line, marked `// Folderstorm:`):

- `folderstorm/mod.rs`: `pub mod gesture_boards;` (fork file).
- `app.rs`: `mod folderstorm_gesture_boards;` and one call beside
  `self.show_gestures(...)`.
- `settings.rs`: the `folderstorm_gesture_boards` field, its default, and
  kept on "reset to defaults" (user content, as layouts).
- `ui/bars.rs`: the World menu entry (beside the Layouts submenu hook), the
  toolbar command's arm in `run_command`, and its look (lit while open).
- `ui/toolbar/catalogue.rs`: the `gesture_board` command;
  `ui/toolbar/tests.rs`: the catalogue count.
- `crates/aurora-i18n/locales/{en,fr,ja}.ftl`: a `fs-gesture-board-*` block.
- `docs/TESTING.md`: the demo scenario.

## Data model

`settings.json`, field `folderstorm_gesture_boards`, per account (agent
id), as `ui::gestures::Settings` keeps the Gestures window's preferences:

```text
Store { accounts: { <agent id>: Account } }
Account { boards: [Board], selected: <board id>, search_current: bool }
Board   { id, name, tiles: [Tile], tile: [w, h], full_names: bool, per_page: 0 | n }
Tile    { id, item: <inventory item id>, label?, color?: [r, g, b], size?: [w, h] }
```

- Tiles keep **inventory item ids only**, never asset ids or anything read
  from the asset. The kind (gesture or sound) and the name are read from the
  inventory when drawn and when played; a link is followed then.
- `id`s are numbers made by the store (unique in the account), so that a
  rename keeps the board and a reorder keeps the tile.
- Limits: 32 boards, 1,024 tiles a board (Folderstorm (C++)'s), names and
  labels up to 64 characters, sizes within 48–480 × 24–240 points.
- Reading never fails: an entry that cannot be read, or breaks a limit, is
  dropped from the board shown but kept in the file (`other` / `kept`), as
  the layouts' store does, so that a newer version's boards survive.

## Decisions (to confirm)

1. **Sounds play in-world, as the inventory's double-click does.** Confirmed
   by the human (2026-11-15): sounds and gestures play publicly, like
   Folderstorm (C++), respecting RLVa and permissions on that path. The
   Legacy Viewer's inventory "Play" on a sound (LLSoundBridge::performAction
   "sound_play", and its double-click) calls `send_sound_trigger`: everyone
   nearby hears it. Aurora ports it as `InvAction::TriggerSound`, and
   Folderstorm (C++)'s board uses that same path ("one-shot in-world audio
   at normal gain"). The board uses `InvAction::TriggerSound` too. RLVa has
   no restriction on sound triggers (Aurora's `world::rlva` and RLVa's
   command list have none): the path is the inventory's own, with the same
   rights (the user's own sound, not the Library's, not the Trash's; the
   asset id is never shown). The tooltip and the help say that others
   hear it.
2. **Gestures play through the Gestures window's Play**
   (`gestures::Action::Play`): it activates an inactive gesture, loads it,
   plays it, and stops it when it is already playing. RLVa `@sendgesture`
   and the "gestures enabled" preference keep their checks
   (`App::gesture_play`); a blocked tile says why in its tooltip.
3. **Items from the user's own inventory only**: not the Library, not the
   Trash. A gesture of the Library cannot be activated anyway (the Gestures
   window leaves them out); stricter than the Legacy Viewer for sounds.
4. **Missing items stay on the board**, dimmed, "Not in your inventory";
   the board asks the server for them once (`FetchItems`), as the gestures'
   code does for the Current Outfit's gestures, and they come back when the
   inventory has them.
5. **Adding an item already on the board** does not add it again: its page
   is shown (Folderstorm (C++)). The same item can be on several boards.
6. **Pages fit the window** (Folderstorm (C++)): as many tiles as the window
   holds, packed in rows in order; a board can also cap a page
   ("Buttons per page": automatic or 4 to 200). Previous / next and "2 / 5"
   show only when there is more than one page.
7. **Search** (Ctrl+F, or the magnifier) lists the tiles whose label or
   item name has the text, as tiles of the same look, under their board's
   name; a click plays. "This board only" is a toggle beside the field
   (remembered per account). Esc closes the search and goes back to the
   page. Ctrl+F follows `ui::find_shortcut` (the window under the pointer
   or with the keyboard takes it).
8. **Editing** a tile (right-click › Edit…) opens a dialog: label (empty:
   the item's name), colour (theme, a swatch of the palette, or any colour
   with the colour picker), size (the board's, or its own). Save / Cancel.
   Board settings (Options): button size for all, "Show full names",
   buttons per page, "Give every button the board's size".
9. **Reorder** by dragging a tile onto another (it goes before it), or
   right-click › Move earlier / Move later. Dropping an inventory gesture or
   sound adds it (at the tile it is dropped on, else at the end); so does
   dropping whole folders, or a selection of several entries (see "Folders
   dropped on a board").
10. **Not ported now**: workspace association, bulk selection and edit,
    "defaults for all boards", keyboard shortcuts per tile (excluded in
    Folderstorm (C++) too), Stop board gestures as a separate button (a
    click on a playing gesture's tile stops it, as in the Gestures window).

## Folders dropped on a board

The human's request (2026-11-16): sounds and gestures can be added by
dragging whole folders, with their sub-folders, onto a board.

- **What is carried.** The Inventory windows (main, additional, folder
  windows; Aurora and Legacy styles share the rows' drag source,
  `ui/inventory/drag.rs` `source`) carry one item (`InvDrag`), one folder
  (`FolderDrag`) or a selection of several entries (a private
  `SelectionDrag`). The board reads all three with
  `inventory::drag::carried_ids` (no upstream change). One item dropped
  behaves as before; a folder, a link to a folder, or several entries go
  through `folders::collect`.
- **The walk.** Each folder and all its sub-folders, depth first. Order,
  stable whatever order the inventory or the selection come in: the entries
  dropped by name; in each folder its own items by name (natural order, as
  the inventory sorts), then each sub-folder by name with all it holds. The
  first folder's direct gestures come first, which reads as the folder
  looks once opened.
- **What goes on the board**: the same rule as one item dropped
  (`gesture_boards::resolve`): gestures and sounds of the user's own
  inventory, a link followed (the tile keeps the link's id, as for one link
  dropped); never the Library's nor the Trash's. A broken link is left out.
  Links to folders are not followed (as LLInventoryModel::collectDescendents),
  so a walk never loops. The Trash met inside a folder (the whole inventory
  dropped) is skipped. Other items are left out silently, only counted.
- **Duplicates.** Stricter than one item dropped: an item is "already there"
  when the board has a tile of the same item **or one that plays the same
  original** (a gesture and a link to it), and the same original found twice
  in one drop goes on once.
- **The limit.** What fits under the 1,024 buttons of a board is added, in
  the walk's order; the rest is counted as "left out: the board is full".
  The walk itself stops after 5,000 folders or 50,000 items (said in the
  summary).
- **Folders not loaded yet.** What is known is added at once; the folders of
  the user's inventory whose contents are not loaded are asked for (the
  window's `Request::FetchFolders`, which the app queues as the Inventory
  window queues the folders it opens), and the summary says to drop the
  folder again in a moment to add the rest: a second drop adds only what is
  new. Not waited for: the simplest correct choice, without a pending drop
  that would add buttons later, after the user has moved on. The Library's
  and the Trash's folders are never fetched for this (the board refuses
  their items anyway).
- **Feedback.** While inventory is carried over the board, the board is
  outlined in the drop colour when the drop can add something (one of its
  items is a gesture or a sound the board takes, or one of its folders is of
  the user's own inventory), in the refusal colour otherwise (the
  Inventory's own `drop_outline` colours), with the Copy or the Not allowed
  cursor (`inventory::drag::feedback`). After the drop, the board's message
  line gives a translated summary: added, already there, left out (board
  full), not allowed (Library, Trash, broken link), other items ignored,
  folders still loading; the first button added flashes and its page is
  shown. A long summary stays longer (at least 5 s) and shows whole in its
  tooltip.
- **Permissions.** Nothing new is read from an asset: the tiles keep item
  ids, play through the paths of the single-item add (decisions 1–3), and
  the walk applies the same rule (`resolve`) to each item.

## Checkpoints

1. Plan (this file), FEATURES 🔧.
2. Store and layout (pure), with tests.
3. Window, dialogs, app glue, hooks, i18n, demo; frame tests and pictures.
4. Checks, FEATURES and TESTING.md.

## Test on Windows and the grid (the human)

See the PR description: add by drag (items, folders with sub-folders,
selections) and by picker, play gestures (active,
inactive, already playing) and sounds (heard by an alt), RLVa
`@sendgesture`, gestures turned off, a deleted item, relog (boards kept per
account), resize the window and the buttons, full names, pages, Ctrl+F on
all boards and on one.
