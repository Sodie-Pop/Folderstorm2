# Multiple Inventory windows: plan

Plan for [FEATURES.md](../FEATURES.md) row **2.14** (the next free number:
`main` stops at 2.12, `claude/inventory-quick-access` adds 2.13). The human's
goal: up to **16 additional full Inventory windows**, each the whole
inventory with its own tabs, search, filters, place and size, as Folderstorm
(C++) had ("up to 16 extra Inventory windows", row 1.4).

Proposed FEATURES row:

| # | Feature | What it does | Aurora today | Source | Prio | Status |
|---|---|---|---|---|---|---|
| 2.14 | Multiple Inventory windows | Up to 16 additional Inventory windows, each the whole inventory with its own tabs, search, filters, place and size ("Inventory 2" … "Inventory 17"). Ctrl+Shift+I and the "New Inventory window" button (in the main window and in every additional window) open the next one; right click on that button: *Close all additional windows*. Layouts and workspaces place and open each one. | One additional window (fixed id) | `doc/help.md` "Workspaces"; Legacy `LLPanelMainInventory::newWindow` | P2 | 📝 plan: [plans/inventory-multiple-windows.md](plans/inventory-multiple-windows.md) |

## 0. Decisions made by the human (2026-11-16)

1. **16 in all**, Aurora's current additional window included. The cap costs
   nothing by itself: only open windows cost memory and frame time, so a
   lower number can be chosen later if the grid test shows a problem.
2. **The button is repurposed**: the main window's "Additional inventory
   window" button no longer toggles one window; it **opens the next free
   slot**, and the same button is shown **in every additional window** too.
3. **Titles numbered**: "Inventory 2" … "Inventory 17"; the main window stays
   "Inventory".
4. **A closed window forgets** its tabs and search (keeps its place), as
   today and in the Legacy Viewer.
5. **At 16 open**: nothing opens; a notice says so; the button is greyed with
   the same tooltip.
6. **Shortcut label** in Preferences › Shortcuts becomes "New Inventory
   window".
7. **Base**: an up-to-date `main` once #74 and #75 are merged.
8. **Performance**: the human tests 16 windows on a large inventory on the
   grid.

The "to confirm" marks below are settled by this list where they overlap.

## Status (2026-11-16): built on branch `claude/inventory-extra-windows`

Built as planned, on `main` with #62/#64 and #74/#75 merged. Where the
build differs from or settles the sections below:

- **Code**: `ui/inventory/extra_windows.rs` (slots, ids, titles, first
  places, opening and closing, the button and its menu, the demo) and its
  `tests.rs`.
- **The button in an additional window** cannot open a window itself (each
  window's state is its own `InventoryUi`; the windows are the main one's).
  The button and its menu leave a request in egui's memory; the main
  window's `show` does it at its next frame (`extra_windows::frame`, one
  hook beside `custom_tabs::window::open_requested`), and tells the buttons
  how many slots are open (tooltip, light, grey).
- **Slot 1 opens through Aurora's `open_additional_window`** (open, or to
  the front), so that upstream function stays in use (the Windows CI job
  denies unused code); `additional_open` and `toggle_additional_window`
  now mean slot 1 (two one-line hooks).
- **Greyed at 16**: Legacy style `widgets::icon_button` disabled; Aurora
  style its tool button in a disabled `Ui` (tooltip kept with
  `on_disabled_hover_text`). The right click still opens the menu, so
  *Close all* stays at hand at 16.
- **Right-click menu**: *New Inventory window* (greyed at 16, the same
  tooltip) and *Close all additional windows* (greyed when none is open).
- **Tab windows (#75)** are not slots (their own ids, `additional: false`)
  and get **no button**: a tab window is locked to one tab, often in quiet
  mode without a toolbar; a whole-inventory button there would be out of
  place. They still close with their tab and under `@showinv`, not with
  *Close all additional windows*. Folder windows: no button either, as
  before.
- **Pin and quiet mode (#75)**: `window_mode::Key::Additional(slot)`, slot 1
  `"additional"` (modes saved before still apply), slots 2-16
  `"additional:<n>"`.
- **Workspaces and layouts**: `folderstorm/workspaces/windows.rs` lists the
  16 slots (`INVENTORY_SLOTS`, checked against `extra_windows::floater_of`);
  their label is the window's title, so slot 1 now reads "Inventory 2" in
  Preferences instead of "Additional inventory window". The `Shell` opens,
  closes and checks each slot by itself (`folderstorm_open_slot`,
  `folderstorm_close_slot`, `folderstorm_slot_open`), not under `@showinv`.
  **Bug fixed**: a workspace closing the additional window used to call
  `close_additional_window`, which closed every additional and tab window.
- **Upstream test** `the_main_window_toggles_one_additional_inventory_window`:
  three marked changes (the additional window has the button; the second
  click opens another window; both closed by hand).
- **Demo**: `AURORA_DEMO_INVENTORY=extra-windows|aurora-extra-windows|extra-windows-full`
  (a per-frame call beside `folder_colors_demo_frame` in `app.rs`).

## 1. What it adds

1. **Up to 16 additional Inventory windows** ("slots" 1 to 16). Each is
   Aurora's additional window as it is today: the whole inventory, its tabs
   (no Received tab), its own search, filters, selection and scroll, both
   styles (Legacy and Aurora).
2. **Each slot keeps its place and size** between openings and between
   sessions, as the additional window does today.
3. **Ctrl+Shift+I opens the next free slot**, as the Legacy Viewer's *New
   Inventory Window* adds a window each time.
4. **Numbered titles**, so the windows can be told apart (in the title bar,
   in Ctrl+W order, in the workspace lists).
5. **Layouts and workspaces** place, open and close each slot.

## 2. How Folderstorm (C++) and the Legacy Viewer do it

Read from the sources (fetched from GitHub's raw files: Firestorm `master`,
Folderstorm (C++) `main` docs).

| What | Where | Behaviour |
|---|---|---|
| *New Inventory Window* (Ctrl+Shift+I) | `llpanelmaininventory.cpp` `LLPanelMainInventory::newWindow`, `get_instance_num` | Each call opens a **new** `inventory` floater keyed by a counter (`instance_num + 1`, no limit). Nothing in mouselook |
| Its menu entries | `menu_viewer.xml` `NewInventoryWindow` (`visible="false"`: shortcut only); `menu_inventory_gear_default.xml` `new_window` (the Inventory gear menu) | So in the Legacy Viewer it is the shortcut, or the gear menu |
| The main window's `new_inv_btn` ("Additional Inventory Window") | `panel_main_inventory.xml`, `Button.SetDockableFloaterToggle` `secondary_inventory`; `LLSidepanelInventory::createSecondaryInventoryWindow` | A **toggle** for one separate floater, `secondary_inventory` |
| Closing | `llfloatersidepanelcontainer.cpp` `closeFloater` | Keyed `inventory` windows and `secondary_inventory` are **destroyed** on close: search and folders forgotten |
| Placement | `LLFloater::applyRectControl` (quoted in `doc/plans/complete-workspaces.md`) | Extra windows cascade in the `inventory` group |
| Folderstorm (C++) workspaces | `doc/help.md` "Workspaces" | "Additional open Inventory windows are saved too (up to 16, plus the workspace-owned extra). Loading restores their size, position, visibility and minimized state, reuses existing windows and hides extras absent from that workspace." |
| Folderstorm (C++) folder context | `doc/plans/workspace-enhancements.md` (Status, line ~71) | Remembered folders cover "the primary, owned extra and ordered ordinary additional Inventory slots" |

So **the 16 of Folderstorm (C++) is the number of additional windows a
workspace saves, in ordered slots**; Firestorm itself opens any number.
Whether Folderstorm (C++) also refused a 17th window is **unverified** (its
code was not read, only its docs).

Aurora merges the two Legacy mechanisms into one window: its comment on
`open_additional_window` says "Aurora keeps one, where the Legacy Viewer adds
another".

## 3. What Aurora has (verified on `origin/main`, `061cf522`)

| What | Where | What it means for us |
|---|---|---|
| `InventoryWindow { id, root, additional: bool, state: Box<InventoryUi>, open }`; the windows are `InventoryUi::windows: Vec<InventoryWindow>` | `ui/inventory.rs:428-437`, `:612` | Each window already has its own whole `InventoryUi` (tabs, search, filters) |
| `ADDITIONAL_WINDOW` fixed id `0x6b1f_2c4e_9a37_4d58_8e21_0f3c_5a90_1090`; `floater_id(id) = "inventory_{id}"` | `ui/inventory.rs:441`, `:446` | The floater id names the window's place in egui's memory |
| Places and sizes are egui memory, written to `ui_layout.ron` 2 s after a change and at exit, read at start | `ui/layout.rs:1-60` | **A deterministic id per slot is all it takes** for each slot to keep its place across sessions |
| `additional_open()`: any `w.additional && w.open` | `ui/inventory.rs:736` | Read by the button (lit), `open_additional_window`, upstream tests, workspaces (#62) |
| `toggle_additional_window`: finds the **first** `w.additional` entry (a closed one kept by a dialog is reused), else pushes a new one with `ADDITIONAL_WINDOW` | `ui/inventory.rs:748-764` | With several slots, "the first" would be any slot: needs a one-line fix |
| `open_additional_window`: brings it forward if open, else toggles | `ui/inventory.rs:769-775` | Called by Ctrl+Shift+I (`app.rs:4902`) and by workspaces (#62) |
| `close_additional_window`: closes every `w.additional` | `ui/inventory.rs:779-783`; called by RLVa `@showinv` every frame (`app/rlva.rs:141-144`) | Closes all slots **with no change** |
| Drawing: `if w.additional` → `Floater::new(&window_id, tr!("common-inventory"), additional_window_pos(ctx, screen), DEFAULT_SIZE)`, `fetch_whole_inventory`; Legacy style `inventory_contents`, Aurora style item count and sync state in the title | `ui/inventory.rs:2368-2375`, `:2388`, `:2411` | Title "Inventory" today, the same as the main window. First place: left of the main window (`additional_window_pos`, `:2120`), then where the resident left it |
| `w.additional` is read **only** in `ui/inventory.rs` (and `progress.rs:293`, a test) | `git grep` | All slots can be `additional: true` and be drawn exactly like today |
| A closed window is dropped at the end of the frame (`windows.retain`), unless a document, dialog, Move to, thumbnail or bulk permissions it opened is still up | `ui/inventory.rs:2505-2513` | Closing forgets folders and search, as the Legacy Viewer destroys it; the place stays (egui memory) |
| `InventoryUi` is not serialized (`#[derive(Default)]` only); only `Panels::inventory` (the main window) is a setting | `ui/inventory.rs:501` | **The additional window is not reopened at login** (confirmed) |
| The button (`ADDITIONAL_ICON` "briefcase-2-filled"), main window only (`is_main`), lit by `additional_open()`, click = `toggle_additional_window` | Legacy: `ui/inventory.rs:1528-1545`; Aurora: `aurora/toolbar.rs:349-364` (drawn at `:415`, `:500`, `:527`) | Both return an `egui::Response`: a right-click menu is one line each |
| Keyboard: `Action::NewInventoryWindow`, Ctrl+Shift+I, label `inventory-additional-window` in Preferences › Shortcuts › Windows | `keybinds.rs:390-392`, `:600`, `:839`, `:1789`; dispatch `app.rs:4902` | **No Me menu entry** in Aurora (`ui/bars.rs:877` Me menu has none; as in Firestorm, where it is hidden), no toolbar catalogue entry (`ui/toolbar/catalogue.rs`), no gear menu entry |
| Ctrl+W / Ctrl+Shift+W close the frontmost / every `widgets::Floater` | `ui/close_key.rs` | Works for every slot, no hook |
| Cascade groups (`widgets/cascade.rs`, `Group::Inventory(kind)`) are for previews, not for these windows | `ui/widgets/cascade.rs` | We offset the first place ourselves, no upstream change |
| Demo `AURORA_DEMO_INVENTORY=additional|aurora-additional` | `app.rs:21188-21189`, `:21268-21273`; `aurora.rs:146-151`; `docs/TESTING.md:11055` | Kept as is |
| Layouts (merged checkpoints 3-5): the allowlist has `ADDITIONAL_INVENTORY = "inventory_6b1f2c4e-…1090"`; a test checks the string against `inventory.rs` | `folderstorm/workspaces/windows.rs:42-56`, `:144-152`; store limit `MAX_WINDOWS = 64` (`store.rs:21`), 22 entries today | Slots 2-16 become 15 more entries (37 ≤ 64) |
| Local assistant: reads every window's `state.preview` (open documents) | `app/local_assistant.rs:58-71` | Iterates all windows already: **no MCP change** |

## 4. Decisions (to confirm)

### Slots and ids

| Point | Decision |
|---|---|
| How many | **16 additional windows in all, Aurora's current one included** (slot 1). To confirm: 16 *beyond* Aurora's (17) is equally easy |
| Ids | Slot 1 = `ADDITIONAL_WINDOW` (unchanged: saved places, layouts, workspaces, upstream tests and demo keep working). Slot *n* = `ADDITIONAL_WINDOW + (n - 1)`, i.e. `…5a90_1091` to `…5a90_109f`: deterministic, so each slot's place and size persist in `ui_layout.ron` |
| The `additional` flag | `true` for every slot: they are drawn, fetched, closed by RLVa exactly like today's window. The slot is derived from `w.id` in our module: **no new field** in the upstream struct |
| Lifetime when closed | As today and as the Legacy Viewer: the window is dropped, its search, folders and tab forgotten; place and size kept. To confirm: keeping each slot's state for the session is possible but needs a hook in the `retain` (more upstream change) |
| At login | **Not reopened**, as today. A workspace can reopen them (row 1.4) |

### Opening

| Point | Decision |
|---|---|
| Ctrl+Shift+I (*New Inventory window*) | Opens the **lowest free slot** (slot 1 first, so with one window it behaves as today). A slot whose closed entry is still kept by a dialog is reused (as `toggle_additional_window` does) |
| The button (decided) | **Repurposed**: a click opens the lowest free slot (it no longer toggles slot 1; the Legacy Viewer's `new_inv_btn` toggle is not kept). Shown in the main window **and in every additional window** (today `is_main` only), both styles. Tooltip "New Inventory window (n of 16 open)". Lit while any slot is open |
| Right click on the button | *Close all additional windows* (slots only, not folder or tab windows) |
| At 16 open | Ctrl+Shift+I opens nothing and shows the Inventory notice "All 16 additional Inventory windows are open" (`InventoryUi::notice`, shown in every Inventory window's status line); the right-click entry is greyed with the same tooltip. To confirm: or bring the most recently opened slot to the front instead |
| First place of a new slot | Slot 1: as today (left of the main window). Slot *n*: slot 1's first place offset by (n − 1) × 24 px down-right, kept inside the screen. After that, where the resident left it |
| Keyboard label | Preferences › Shortcuts keeps upstream's "Additional inventory window" label, or (one-line hook, to confirm) "New Inventory window" |
| Mouselook | Not ported (the Legacy Viewer opens nothing in mouselook; Aurora does not check today) |

### Titles

| Point | Decision |
|---|---|
| Titles | Slot *n* is **"Inventory *n + 1*"** ("Inventory 2" … "Inventory 17"); the main window stays "Inventory". To confirm: or keep slot 1 "Inventory" as today and number only slots 2-16 |
| Aurora style | The item count and sync dot stay after the title, as today |
| Text | `tr!("inventory-windows-title", number = …)` with en / fr / ja |

### Closing

| Point | Decision |
|---|---|
| Cross, Ctrl+W | Closes that slot only (no hook: `widgets::Floater`) |
| Ctrl+Shift+W | Closes all windows, slots included (no hook) |
| *Close all additional windows* | Closes slots 1-16, not folder windows nor tab windows |
| RLVa `@showinv` | `close_additional_window` already closes every `additional` window: all slots, every frame while restricted. No change |

## 5. Where the code goes

- `crates/aurora-viewer/src/ui/inventory/extra_windows.rs` (new): the slot
  numbers and ids (`slot_of(id)`, `id_of(slot)`, `MAX_SLOTS = 16`), titles,
  first place, `impl InventoryUi { folderstorm_new_window,
  folderstorm_open_slot, folderstorm_close_slot, folderstorm_slot_open,
  folderstorm_close_all_slots }`, the right-click menu of the button, the
  "all open" notice, the demo. A child module of `ui/inventory`, so it reads
  the private `ADDITIONAL_WINDOW`, `floater_id`, `additional_window_pos` and
  `DEFAULT_SIZE` with no visibility change.
- `crates/aurora-viewer/src/ui/inventory/extra_windows/tests.rs` (new).
- `crates/aurora-viewer/src/folderstorm/workspaces/windows.rs` (ours): 15
  allowlist entries for slots 2-16 (literal `"inventory_…1091"` … `"…109f"`,
  checked by a test against `extra_windows::id_of`). Their label needs a
  number: `label` becomes `fn() -> String` (or the entry gains a slot field).
- `crates/aurora-viewer/src/app/folderstorm_workspaces.rs` (ours, once #62 is
  in `main`): the `Shell` arms per slot (section 8).
- `crates/aurora-viewer/src/ui/inventory/window_mode.rs` (ours, once #75 is
  in `main`): a key per slot (section 8).
- Locales: `inventory-windows-*` keys in `en.ftl`, `fr.ftl`, `ja.ftl`.

## 6. Upstream hooks (each marked `// Folderstorm:`; lines on `origin/main`)

| File | Line | Hook |
|---|---|---|
| `ui/inventory.rs` | ~78 (module list) | `mod extra_windows; // Folderstorm: up to 16 additional Inventory windows` |
| `ui/inventory.rs` | 737 `additional_open` | `w.additional && w.open` → `w.id == ADDITIONAL_WINDOW && w.open`: "the additional window" now means slot 1 (the button's light, `open_additional_window`, workspaces) |
| `ui/inventory.rs` | 751 `toggle_additional_window` | `.find(\|w\| w.additional)` → `.find(\|w\| w.id == ADDITIONAL_WINDOW)`: the button toggles slot 1, not whichever slot comes first |
| `ui/inventory.rs` | ~2368-2375 (`if w.additional` → `Floater::new`) | Title and first place from our module: `&extra_windows::title(w.id)` instead of `tr!("common-inventory")`, `extra_windows::first_pos(w.id, additional_window_pos(ctx, screen), screen)` instead of `additional_window_pos(ctx, screen)` (2 lines) |
| `ui/inventory.rs` | ~1503 and ~1541-1544 (Legacy `additional_button` and its call) | The button's click calls `st.folderstorm_new_window(ctx)` instead of `toggle_additional_window`, its tooltip, greyed state and right-click menu come from our module (`extra_windows::button(…)`), and the `is_main` condition around its call also lets additional windows draw it (2-3 lines) |
| `ui/inventory/aurora/toolbar.rs` | ~349-364 and its calls at ~415, ~500, ~527 (Aurora `additional_button`) | Same as the Legacy style: click opens the next slot, our tooltip / greyed state / menu, drawn in additional windows too (2-3 lines) |
| `app.rs` | 4902 | `Action::NewInventoryWindow => self.inventory_ui.folderstorm_new_window(&self.egui_ctx), // Folderstorm` |
| `app.rs` | ~21207 and ~21230 (demo names and calls) | `\| "extra-windows" \| "aurora-extra-windows" \| "extra-windows-full"` and one `if view.contains("extra-windows")` call |
| `keybinds.rs` | 600 | The label "New Inventory window" (decided; a new `inventory-windows-new` key, upstream's key left alone) |
| `docs/TESTING.md` | after `aurora-additional` (~11055) | The demo's entry |

Not touched: `open_additional_window` (still "slot 1 open or to the front":
#62 relies on it), `close_additional_window` (already all slots),
`app/rlva.rs`, `aurora.rs`, `widgets.rs`, `layout.rs`, the `InventoryWindow`
struct.

## 7. Settings and storage

- **No new setting.** Places and sizes: egui memory per floater id
  (`ui_layout.ron`), as today. Which slots are open: not saved (not reopened
  at login).
- Workspaces (#62) save open slots as allowlisted window ids in their `open`
  list; layouts save their rectangles. Both are existing formats.
- With #75: pin and quiet mode per slot in `inventory.window_modes`
  (section 8).

## 8. Interaction with open PRs

### #74 / #75 (custom tab folders, Inventory quick access)

#75 changes **the same lines**: it adds the tab-window branch before
`if w.additional` at `Floater::new` (~2389), `w.additional ||
tab_window.is_some()` in the Legacy draw, and makes `close_additional_window`
close tab windows too. Its `window_mode::Key::Additional` (`"additional"`)
would give **one pin / quiet mode to all 16 slots**.

When both are in:

- `Key::Additional` becomes `Key::Additional(slot)`: `"additional"` for slot
  1 (saved modes kept), `"additional:<n>"` for slots 2-16;
  `Key::of_window` takes the slot from `w.id`.
- Tab windows have `additional: false` and their own ids (`0x7a3c_…`): no
  clash with slots.
- *Close all additional windows* closes slots only, not tab windows.

### #62 / #64 / #72 (workspaces)

#62 knows the additional window by its fixed id and calls
`open_additional_window`, `close_additional_window` and `additional_open`
(`app/folderstorm_workspaces.rs` `Windows::available`, `is_open`, `open`,
`close`, branch `claude/workspaces-cp6-folders` lines ~71-131). #64 lists
folder windows with `!w.additional && !w.root.is_nil()`
(`ui/inventory/folderstorm_windows.rs`): slots are never taken for folder
windows. #72 (context) and #73 do not touch Inventory windows.

How N slots are saved and restored by a workspace: **each slot is one more
allowlisted window**, so the existing machinery does it.

- Save: the open slots' ids go in the workspace's `open` list; their rects
  in its layout.
- Restore: open exactly the listed slots, close the other open slots ("reuses
  existing windows and hides extras absent from that workspace", as
  Folderstorm (C++)). Old workspaces (slot 1 = the old id) keep working.
- `Shell` arms: `is_open` → `folderstorm_slot_open(n)`; `open` →
  `folderstorm_open_slot(ctx, n)`; `close` → `folderstorm_close_slot(n)`;
  `available` → not under `@showinv`.
- **Bug to fix on the way**: `Shell::close(ADDITIONAL_INVENTORY)` calls
  `close_additional_window`, which closes every slot (and, with #75, every
  tab window). It must close slot 1 only.
- Remembered folders (#64) for the additional slots (Folderstorm (C++)
  remembered one selected All Items folder per normal window): **later**, a
  follow-up, not in this PR.

### Layouts (checkpoints 3-5, merged)

The allowlist (`windows.rs`) has the additional window; slots 2-16 join it,
so layouts capture and place them. Layouts on `main` do not open windows
(#57 adds that through `Shell::open`: same arms as above).

### Which branch to base on

**From an up-to-date `main`, after #74 / #75 are merged** (they are older and
change the same lines; the rules want one feature per branch, from `main`).

- If #75 is still open when work starts: base on `main` anyway, and adapt
  `window_mode::Key` in whichever PR merges second (a few lines, our files).
- If #62 / #64 merge first: this PR adds the `Shell` arms. Otherwise the
  slots' allowlist entries land here and the workspace PR adds the arms at
  its next merge of `main`.
- Not stacked on #75 or #64: that would tie this PR to two unvalidated
  stacks.

## 9. RLVa

`@showinv`: `app/rlva.rs:141-144` calls `close_additional_window` every frame
while restricted; it closes every `additional` window, so all slots. No
change. Ctrl+Shift+I under `@showinv` opens a slot that RLVa closes the next
frame, as today. Workspaces skip slots under `@showinv` (`available`).

## 10. Demo

`AURORA_DEMO_INVENTORY=extra-windows|aurora-extra-windows`: the main window,
slots 1, 2 and 3 open and cascaded, slot 2 on another tab with a search
typed, slot 3 minimized. `extra-windows-full`: all 16 slots open, the notice
"All 16 additional Inventory windows are open" and the button's right-click
menu open with *New Inventory window* greyed.

## 11. Tests

- Ids: slot 1 is `ADDITIONAL_WINDOW`; 16 distinct ids; `slot_of(id_of(n)) ==
  n`; the workspace allowlist strings equal `floater_id(id_of(n))`.
- Ctrl+Shift+I opens slots 1, 2, 3 in order; after closing slot 2 the next
  opens slot 2; at 16 nothing opens and the notice is set.
- The button opens the next free slot, from the main window and from an
  additional window, in both styles; greyed at 16; its light follows "any
  slot open".
- `close_additional_window` (RLVa) closes all 16.
- Titles "Inventory 2" … "Inventory 17"; first places cascade and stay on
  screen.
- A slot closed and reopened is fresh (tab and search forgotten) and keeps
  its place (headless `egui::Context`, area state).
- Two slots keep separate searches and tabs (headless draw, both styles).
- Right-click menu: *New Inventory window* opens the next slot, greyed at 16;
  *Close all* closes slots only (not folder windows).
- The upstream test `the_main_window_toggles_one_additional_inventory_window`
  tests the old toggle, which the repurposed button replaces: it gets a
  `// Folderstorm:` change (the second click opens slot 2 instead of closing
  slot 1), or is skipped by a small cfg-free rewrite; to settle while
  building, keeping the upstream diff as small as possible.
- With #75: `Key::Additional(n)` text round trip, slot 1 stays
  `"additional"`. With #62: `Shell` opens and closes one slot, not the others.

## 12. Local assistant (MCP)

None. The bridge has no window tools; `local_assistant_open_documents`
already reads every window of `windows`.

## 13. Risks and open questions

- **Performance**: 16 full windows each keep their own view cache and run
  their own search and list every frame; with a large inventory (tens of
  thousands of items) this may cost frame time. Not measured. To try in demo
  with `AURORA_DEMO_INVENTORY=large` plus the slots, then on the grid.
- **Upstream merges**: the hooks are in Aurora's most active file
  (`ui/inventory.rs`). If upstream reworks the additional window (for
  example turns it into several windows itself), we drop ours.
- **Title change of slot 1** ("Inventory" → "Inventory 2"): decided.
- **The repurposed button** changes upstream behaviour (no more toggle) and
  one upstream test; the upstream diff stays a few lines in two files.
- **Workspace close bug** (section 8) exists today between #62 and #75,
  independent of this feature.
- **Unverified**: whether Folderstorm (C++) capped opening at 16 or only the
  workspace capture (its code was not read); frame timing of the demo
  placement against `aurora::demo_place`.
