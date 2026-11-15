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

1. **Sounds play in-world, as the inventory's double-click does.** The
   Legacy Viewer's inventory "Play" on a sound (LLSoundBridge::performAction
   "sound_play", and its double-click) calls `send_sound_trigger`: everyone
   nearby hears it. Aurora ports it as `InvAction::TriggerSound`, and
   Folderstorm (C++)'s board uses that same path ("one-shot in-world audio
   at normal gain"). The board uses `InvAction::TriggerSound` too. RLVa has
   no restriction on sound triggers; the board adds none. The tooltip says
   that others hear it.
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
   sound adds it (at the tile it is dropped on, else at the end).
10. **Not ported now**: workspace association, bulk selection and edit,
    "defaults for all boards", keyboard shortcuts per tile (excluded in
    Folderstorm (C++) too), Stop board gestures as a separate button (a
    click on a playing gesture's tile stops it, as in the Gestures window).

## Checkpoints

1. Plan (this file), FEATURES 🔧.
2. Store and layout (pure), with tests.
3. Window, dialogs, app glue, hooks, i18n, demo; frame tests and pictures.
4. Checks, FEATURES and TESTING.md.

## Test on Windows and the grid (the human)

See the PR description: add by drag and by picker, play gestures (active,
inactive, already playing) and sounds (heard by an alt), RLVa
`@sendgesture`, gestures turned off, a deleted item, relog (boards kept per
account), resize the window and the buttons, full names, pages, Ctrl+F on
all boards and on one.
