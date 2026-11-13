# Layout profiles and workspaces: plan

Plan for [FEATURES.md](../FEATURES.md) rows **1.2** (the rest: the
interactive viewport diagram), **1.3** layout profiles, **1.4** workspaces and
**1.5** the switcher, with an outline of 1.6–1.9. Planning only: nothing here
is implemented, built or run yet. Written 2026-11-11 from Folderstorm2 `main`
(`45bccb7`, custom world viewport merged) and Folderstorm (C++) `main`
(`abbe3cb0ca`).

Claims about Aurora cite the file they come from (section 4). Claims about
Windows behaviour that a Linux sandbox cannot check are marked
**(verify on Windows)**. Section 17 sorts what was read in code from what was
inferred.

---

## 1. In short

- **A layout says *where*; a workspace says *what*.** A layout holds the
  viewer window (position, size, optionally locked), the world viewport, the
  bars, and the position and size of each window. A workspace holds which
  windows are open, and optionally which layout to use. Any workspace works on
  any layout. (Folderstorm (C++) split them differently; section 3.)
- **Layouts must store each window's rectangle.** When the viewer window
  shrinks, Aurora keeps window *positions* recoverable but overwrites window
  *sizes* for good (section 5). A saved layout is the only copy that survives.
- **One small fix first** (checkpoint 1): floating windows are shrunk to fit
  the *world* instead of the viewer window when the bars sit inside the
  world. This very likely caused the squashed windows the human saw.
  Aurora's start-up rule that reopens a window stretched over two monitors at
  80 % of one monitor is **kept** (confirmed on Windows; the human likes how
  it works together with the viewport fallback): a two-monitor layout
  restores the stretched window in one click instead (checkpoint 4).
- **One click from 2 monitors to 1.** A toolbar toggle flips between two
  chosen layouts; a switcher (type to filter, Enter) handles the rest.
- **Lock the window size** per layout, and a "borderless over these
  monitors" mode that also locks the position. Unlocking is always one click
  away, and automatic when a monitor goes missing.
- **Saved in `settings.json`**, so each `--profile` character (session host
  plan) gets its own layouts for free. Only Inventory folder references are
  per account.
- Six small PRs (section 13). Almost all code lives in
  `folderstorm/workspaces/`; upstream files get about 25 marked hook lines
  (section 10).

## 2. Goals and non-goals

**Goals**

1. Switch the whole screen arrangement in one action: viewer window, world,
   bars and windows. First use case: two monitors ⇄ one monitor.
2. Recover in one action after the viewer window shrank or a monitor went
   missing, with windows back at their real sizes.
3. Optionally lock the viewer window's size (and position) while a layout is
   active.
4. Switch activities ("sorting", "exploring") by opening and closing windows,
   on whichever layout is active.
5. Edit the world viewport on a diagram of the window and the monitors
   (row 1.2).
6. Windows first; Linux (X11) and macOS best effort; Wayland degrades cleanly.

**Non-goals**

- Real OS windows for floaters (egui multi-viewport). Aurora draws every
  window inside one OS window; the stretched window of row 1.1 is the fork's
  model.
- Saving anything a window *contains*: messages, conversations, Inventory
  contents, search filters, selections, map positions.
- A generic "save every window" system. Only an allowlist of windows is
  saved (section 7.3).
- Restoring a workspace automatically at login (Folderstorm (C++) kept that
  manual too; an opt-in can come later).
- Changing Aurora's own window layout file (`ui_layout.ron`) or its anchor
  rules.

## 3. What Folderstorm (C++) did, and what we keep or change

Sources: `doc/help.md` "Wide-screen layout" and "Workspaces";
`doc/plans/visual-viewport-setup.md`, `complete-workspaces.md`,
`workspace-enhancements.md`, `workspace-context-and-snapshot.md`.

| Folderstorm (C++) | Folderstorm2 | Why |
|---|---|---|
| **Layout profile** = viewport margins + bar placement (global setting) | **Layout** = viewer window + viewport + bars + **window rectangles** | The human's main case (2 ⇄ 1 monitor) needs window sizes back; see section 5 |
| **Workspace** = a full snapshot: its own copy of the layout + windows, per account | **Workspace** = which windows are open + an optional layout name | No duplicated geometry; "Sorting" works on either monitor setup |
| Workspaces stored per account | Stored per profile (`settings.json`); folder references per account | Aurora's window layout is global, not per account; `--profile` separates characters |
| Viewport margins in percent | Margins in window pixels (already so in row 1.1), plus the viewer window in the layout | Margins are tied to monitors, which are in pixels |
| Preferences Preview / OK / Cancel transaction | Applied at once; **Previous arrangement** undoes the last switch | Aurora's Preferences apply immediately (`ui-options-changes-apply-immediately`) |
| Diagram: drag edges, pick a monitor, "Use this monitor" | Same, plus edges snap to monitor borders | Kept |
| Switcher: type to filter, Enter, favorites first, Save new with parts, Preview | Same | Kept |
| Keyboard actions with no default keys; Previous arrangement; modified marker; Update current; restore report | Same | Kept |
| Up to 16 extra Inventory windows; optional "Remember Inventory folders" | Aurora's additional window + up to 16 folder windows; same option | Aurora has one additional window and per-folder windows (section 4) |
| Conversations: geometry only, never sessions | Same | Kept |
| Received Items panel size in the main Inventory | Not needed | Received Items is a tab of Aurora's main Inventory window, not a split panel |
| Snapping and alignment tools | Not ported | Aurora already snaps (Alt) and glues windows (`ui/layout/gesture.rs`) |
| Starting arrangements (Inventory sorting, Driving) | Not ported | Folderstorm (C++) removed them too |
| Startup restore, toolbar button sets, export / import, quiet UI | Later, if asked | Keep the first version small |
| Graphics / camera / HUDs in a workspace, monitor recovery, MCP tools | Row 1.7 outline (section 14); missing-monitor handling is in this plan | |

Lessons from the C++ plans worth keeping: never write fitted geometry back
into a saved profile; capture only what you can restore; a stored string must
never open an arbitrary window; a missing piece skips with a note instead of
failing the whole switch.

## 4. What Aurora already has (verified in code)

| What | Where | What it means for us |
|---|---|---|
| Each window keeps one of 8 anchors (corners, side middles) of the "move room" and its offset; when the room changes size, the window keeps that offset. Clamping never rewrites the anchor | `ui/layout.rs` (`Windows::follow`, `Slot`), `ui/layout/anchor.rs` | Positions survive a shrink and come back on growth |
| A resizable floater larger than the work area is shrunk to it, and the new size is recorded (`Placed.rect`; `expanded_size` rewritten each frame) | `ui/widgets.rs` `Floater::show` (`shrink`, line ~947) | **Sizes are lost** when the viewer shrinks |
| The work area is computed inside the bars; with the viewport's "bars inside the world" it is inside the world rectangle | `app.rs` (bars child UI, `set_work_area`) | Windows are shrunk to the world, not the window (section 5) |
| Public helpers to place a window: `place_floater` (position + size, expands it), `move_floater`, `size_floater`, `minimize_floater`, `expand_floater` | `ui/widgets.rs` | We can place windows with no new upstream API. The mini-map (`bare_window`) ignores them: needs a hook |
| Window rectangles live in egui's memory, saved to `ui_layout.ron` 2 s after a change | `ui/layout.rs` (`Autosave`) | `ctx.memory(|m| m.area_rect(id))` reads any known window, open or closed |
| `layout::text` / `layout::apply`: the whole egui memory as RON (used by settings backups) | `ui/layout.rs` | Considered as the layout format and rejected: it also resets scroll positions, expanded folders and text cursors, and egui upgrades can break it |
| Viewer window: logical size, physical outer position, maximized, borderless full screen; `WindowedGeometry::apply` | `settings.rs` (`window_*`), `app.rs` | Model for applying a window frame |
| At start, a saved size covering ≥ 95 % × ≥ 90 % of the monitor holding its top-left corner is reopened at 80 % of that monitor | `app.rs` `resumed()` | A window stretched over two monitors matches this test (section 5) |
| Minimum window size 800 × 560 (logical) | `app/gpu_recovery.rs` `open_window` | Limits what a layout can ask |
| The monitor list and window position, published once a second; none on Wayland | `app/folderstorm_viewport.rs` `publish_monitors` | Source for the diagram and monitor matching |
| Named graphics profiles: a map in `settings.json`, "modified" marker, Save / Save as / Delete, kept by "Reset everything" | `graphics_presets/saved.rs`, `ui/options/saved_graphics.rs`, `settings.rs` `reset_keeping_account` | Pattern for our lists |
| Open windows are flags in `Panels`; some are remembered for the next start | `ui/mod.rs` | A workspace sets these flags |
| Inventory: main window, **one** additional window (fixed id), folder windows (`open_folder_window`, a random id each) | `ui/inventory.rs` | Workspace Inventory part (section 7) |
| RLVa closes Inventory, mini-map and world map every frame while restricted | `app/rlva.rs` | A workspace cannot bypass RLVa by opening them |
| Toolbar commands catalogue and their dispatch; keyboard `Action` enum | `ui/toolbar/catalogue.rs`, `ui/bars.rs` `run_command`, `keybinds.rs` | Hook points for the toggle and shortcuts |
| Viewport: margins in window pixels; world falls back to the full window under 25 %; "Fit to a monitor" buttons | `folderstorm/viewport.rs`, `ui/options/pages/folderstorm_viewport.rs` | Row 1.1, merged |

## 5. What happens today when the viewer window shrinks

The human's report: going from two monitors to one, the world became a sliver
(fixed by `ac903bc`) and the floating windows looked squashed. Reading
`ui/layout.rs` and `ui/widgets.rs`, here is what Aurora does, step by step.

1. **Positions follow an anchor.** Each window stays at the same offset from
   its nearest corner or side middle. Windows on the right monitor were
   anchored to the right edge, so they slide onto the left monitor, over the
   windows already there. That is the "crowded" part.
2. **Sizes are clamped, then forgotten.** A resizable window larger than the
   work area is shrunk to it, and the shrunk size becomes its remembered size.
   When the viewer grows again, the window stays small.
3. **The work area follows the world.** With the bars inside the world (the
   default of row 1.1), the work area is inside the world rectangle. So the
   shrink in step 2 fits windows to the *world*, even windows sitting in the
   margins. Before `ac903bc`, a world reduced to a sliver squashed every
   resizable window to the sliver's width: very likely the squashed windows
   the human saw. Since `ac903bc`, a world under 25 % fills the window, but
   an in-between width still squashes (3840 → 2560 px with a 1920 px margin
   leaves a 640 px world; windows wider than that shrink).
4. **A restart shrinks a stretched window.** At start, a saved size that
   covers at least 95 % of the width and 90 % of the height of the monitor
   holding its top-left corner is treated as "a maximized window recorded as
   normal" and reopened at 80 % of that monitor. A window over two monitors
   side by side passes this test, so it reopens at about 1536 × 864
   **(verify on Windows)**.

**So layouts must store per-window rectangles.** Positions alone would be
recoverable through anchors, but sizes are not: Aurora keeps only the current
rectangle and overwrites it. A layout keeps its own copy (position and size,
and whether the window is minimized), and fitting a layout to a smaller
window never writes back into it. Applying the right layout later gives every
window its real size again.

**When a saved layout does not fit** (smaller window, missing monitor), each
window is fitted for the current session only: same nearest anchor and
offset as Aurora uses, size reduced to the room if needed, title bar kept
reachable (`layout::keep_title_in`). Section 8.5 gives the rule for each part.

## 6. Definitions

| | **Layout** | **Workspace** |
|---|---|---|
| Answers | *Where* things go | *What* is open |
| Viewer window: position, size, maximized, full screen, borderless over monitors, lock | Yes (optional part) | Through its layout, if it names one |
| World viewport (margins, mouselook switch) | Yes (optional part) | Through its layout |
| Bars: inside the world, around it, or over the whole window | Yes (with the viewport) | Through its layout |
| Position, size, minimized state of each allowlisted window, open or closed | Yes (optional part) | — |
| Which windows are open | Never changed | Yes |
| Additional Inventory window, folder windows | Placement only | Opens them; folder roots optional |
| Contents, chat, selections, filters | Never | Never |
| Stored | Per profile | Per profile; folder references per account |

Example. Layouts **"2 monitors"** and **"1 monitor"**. Workspaces
**"Sorting"** (Inventory and the additional Inventory open, maps closed) and
**"Exploring"** (mini-map and world map open, Inventory closed). "Sorting"
on "1 monitor" opens both Inventory windows where "1 monitor" puts them; on
"2 monitors", where that layout puts them. A workspace may also name a layout
("Photos" always uses "1 monitor").

## 7. Data model and storage

### 7.1 Records

All in a new module, `crates/aurora-viewer/src/folderstorm/workspaces/`.
Sketch (field names indicative):

```rust
/// settings.json → `folderstorm_workspaces`
pub struct Store {
    pub layouts: Vec<Layout>,            // at most 32, in the user's order
    pub workspaces: Vec<Workspace>,      // at most 32
    pub active_layout: Option<String>,
    pub active_workspace: Option<String>,
    pub favorites: Vec<Entry>,           // ordered: Entry::Layout(name) | Entry::Workspace(name)
    pub toggle: Option<[String; 2]>,     // the two layouts of the toolbar toggle
    pub on_monitor_change: MonitorPolicy, // Ask (default) | Apply | Nothing
    pub pending: BTreeSet<String>,       // windows placed while closed, not shown since
}

pub struct Layout {
    pub schema: u32,                     // 1
    pub name: String,                    // 1–40 characters, unique ignoring case
    pub window: Option<WindowFrame>,     // None: the viewer window is left alone
    pub world: Option<World>,            // None: viewport and bars left alone
    pub windows: Option<Placements>,     // None: windows left alone
    pub monitors: Vec<Monitor>,          // the setup it was saved on
}

pub enum WindowFrame {                   // desktop pixels
    Normal { x: i32, y: i32, width: u32, height: u32, maximized: bool, locked: bool },
    Fullscreen { monitor: u8 },          // index into `monitors`
    Borderless { monitors: Vec<u8>, locked: bool }, // covers exactly these monitors
}

pub struct World {
    pub viewport: ViewportSettings,      // the existing row 1.1 struct
    pub bars: BarsPlace,                 // Inside | Around | Window
}

pub struct Placements {
    pub frame: Frame,                    // interface size (points) and pixels per point when saved
    pub windows: BTreeMap<String, Placement>, // by window id, at most 64
}
pub struct Placement { pub x: f32, pub y: f32, pub width: f32, pub height: f32, pub minimized: bool }

pub struct Monitor { pub x: i32, pub y: i32, pub width: u32, pub height: u32, pub scale: f32 }

pub struct Workspace {
    pub schema: u32,
    pub name: String,
    pub layout: Option<String>,          // None: keep the current layout
    pub open: BTreeSet<String>,          // allowlisted window ids open in it
    pub additional_inventory: bool,
    pub folders: Option<FolderWindows>,  // "Remember Inventory folders"
}
pub struct FolderWindows {
    pub account: Uuid,                   // applies only when this account is logged in
    pub windows: Vec<(Uuid, Placement)>, // folder id and its window's place, at most 16
}
```

Monitor *names* are shown live but never saved: geometry and scale are
enough to recognise a setup.

### 7.2 Where it is saved

- **`settings.json`**, field `folderstorm_workspaces`, beside the named
  graphics profiles (`graphics_saved`). Written by Aurora's settings saver.
- **Kept by "Reset everything"**, like graphics profiles ("named profiles are
  user content", `Settings::reset_keeping_account`).
- **In settings backups** automatically (the backup's "settings" part).
- **Per profile.** The session host plan's `--profile <name>` moves
  `config_dir()`, so `settings.json` and `ui_layout.ron` follow the profile.
  Each character gets its own layouts with no extra code. A new profile
  starts with none (or with a copy, through the session host plan's profile
  copy).
- **Folder references** carry the account id; another account on the same
  profile skips them with a note.

### 7.3 The window allowlist

Only windows listed in code are saved and restored: Inventory, the additional
Inventory window, Conversations, People, Places, Search, mini-map, world
map, Performance, camera and movement controls, Gestures, Appearance, region
tracker, area search, and a few more checked one by one. Each entry names its
window id and, for workspaces, the `Panels` flag that opens it.

Never saved: Preferences, the switcher itself, dialogs and confirmations,
per-object windows (profiles, item properties, script and notecard editors),
torn-off conversations, notifications.

Unknown ids in a saved file are ignored. A stored string never opens a window
that is not on the list.

### 7.4 Validation

Before anything is applied: numbers finite; sizes above zero and at most
16 384 points; window frame within ±32 768 desktop pixels; names without
control characters; counts within the limits above. A record that fails is
shown as "cannot be used" and is never applied or rewritten. A record from a
newer schema is listed read-only ("made by a newer version").

## 8. Behaviour

### 8.1 Save

- **Save as new…** asks a name and which parts to keep: *Viewer window*,
  *World viewport and bars*, *Window positions* (all ticked by default; a
  layout can be "windows only" or "viewport only").
- The viewer window part records the current state: maximized → `Normal`
  with `maximized`; Aurora's full screen → `Fullscreen` on its monitor; a
  borderless layout → `Borderless`; otherwise `Normal`. A *Lock the window
  size* box goes with it.
- Window positions: every allowlisted window the viewer knows, open or
  closed. Open windows from their live rectangle, closed ones from their
  last remembered rectangle, minimized ones with their expanded size. A
  window never opened has no entry and is left alone later.
- **Update** overwrites the active layout or workspace with the current
  state, after a confirmation naming it.
- Saving a workspace records the open allowlisted windows, the additional
  Inventory window, and, if *Remember Inventory folders* is ticked, each
  folder window's folder and place.

### 8.2 Apply

One action, in steps. Each step waits for the previous one to settle.

1. **Check** the record (section 7.4).
2. **Keep the current arrangement** as *Previous* (memory only).
3. **Viewer window.** Unlock; leave Aurora's full screen if needed; set the
   border, position and size (desktop pixels). Wait until the window reports
   the requested size, 10 frames without change, or 1.5 s.
4. **World viewport and bars.** Write the settings; they take effect on the
   next frame.
5. **Windows.** On the next frame (bars drawn, work area known), place each
   window: open ones at once, closed ones when they next open
   (`move_floater` + `size_floater`; the minimized state is kept).
6. **Workspace only.** Open the windows in its list and close the other
   allowlisted ones, through the same `Panels` flags as the menus. Open the
   additional Inventory window and the folder windows.
7. **Lock** the window if the layout asks and its monitors are all present.
8. **Mark active**, clear the modified marker, save settings. If something
   was skipped or fitted, show one line: "Applied '2 monitors': 1 monitor
   missing, 3 windows fitted."

A new switch cancels the unfinished steps of the previous one. Closed windows
placed in step 5 keep their request until they open; a request still pending
at quit is issued again at the next start.

### 8.3 Switch, Previous, modified

- **Previous arrangement** swaps the current arrangement and the one kept in
  step 2: pressing it again goes back. One level, memory only, cleared at
  quit.
- **Modified**: "• modified" beside the active name when the live state
  differs from it (viewport or bars, window frame by more than 4 px, a window
  by more than 2 points). Checked after a mouse release or a viewer resize,
  at most twice a second, like Aurora's layout autosave.
- **At start** nothing is re-applied: Aurora already restores the window,
  the settings and the window positions (with checkpoint 1's fix, a
  stretched window too). A locked layout is locked again if its monitors are
  all present. Workspaces are never applied at login.

### 8.4 Locking the window size

| Mode | How | What it locks |
|---|---|---|
| *Lock the window size* (Normal frame) | `Window::set_resizable(false)` (winit 0.30): no resize border, no maximize button | Size. The window can still be moved by its title bar |
| *Borderless over monitors…* | `set_decorations(false)`, then position and size equal to the union of the chosen monitors | Size and position (no title bar to drag). Win+Shift+arrows may still move it **(verify on Windows)** |

- Layouts can still resize a locked window: the lock only stops the user's
  mouse.
- **Unlock is always one click away**: World › Layouts › *Unlock the viewer
  window*, the switcher, or applying any layout without a lock.
- **Automatic unlock** when the monitor setup changes so that the layout's
  monitors are not all present, and when entering Aurora's full screen.
- After a GPU recovery the window is recreated (`app/gpu_recovery.rs`): the
  lock and border are applied again.
- Windows: a non-resizable window should also refuse Aero Snap resizing
  **(verify on Windows)**. A borderless window exactly covering monitors may
  be treated as full screen by the compositor; that is fine.
- Linux X11: works through size hints. Wayland: no window position, so
  *Borderless over monitors* is disabled and *Normal* restores the size only.
  macOS: a window cannot span displays while "Displays have separate Spaces"
  is on (the default), so *Borderless over monitors* is limited to one
  monitor there.

### 8.5 Missing monitors, or a layout that does not fit

| Part | Rule |
|---|---|
| Normal window | Moved only if its title bar lands on a connected monitor (the start-up test of `App::resumed`); otherwise left where it is, its size cut to the monitors' union |
| Full screen on monitor N | N missing → the monitor the window is on |
| Borderless over monitors | Uses the chosen monitors still present (matched by position and size); none → left alone; never locked while one is missing |
| World viewport | Applied as saved; the existing rule (world fills the window when the margins leave it under 25 %) protects |
| Window positions | Fitted for this session (section 5); the saved layout keeps its values |

**When the monitors change** (detected from `publish_monitors`, after 2 s
without further change, so that Windows has finished moving the window
itself): with the default *Ask*, a notice offers the layout saved on exactly
this monitor setup: "Your monitors changed. Use layout '1 monitor'?"
[Apply] [Not now]. *Apply* does it without asking; *Nothing* does nothing.
Matching ignores the order of monitors.

### 8.6 DPI and interface scale

- Viewport margins and the viewer window frame are in physical pixels: a DPI
  change does not affect them.
- Window **sizes** are kept in points, so text stays readable. Window
  **positions** go through physical pixels when the pixels-per-point differs
  from the saved one, then are fitted.
- A DPI change while a layout is active (the window dragged to a monitor with
  another scale) is left to Aurora's anchors; the modified check compares in
  physical pixels to avoid a false marker.
- A window spanning monitors with different scales gets one scale from
  Windows (the monitor holding most of it) **(verify on Windows)**; nothing
  special is done.

### 8.7 Login, mouselook, RLVa, Preferences

- The switcher and shortcuts work once logged in. Applying layouts from the
  login screen can come later.
- In mouselook, layouts and workspaces still apply; windows are placed when
  the interface shows again.
- RLVa: windows are opened through `Panels`, and `app/rlva.rs` closes the
  blocked ones every frame. Folder windows are not opened while `@showinv`
  holds. The report says "Inventory: blocked by RLVa".
- Preferences may stay open during a switch (Aurora's Preferences have no
  OK / Cancel). Preferences itself is never captured or moved.

## 9. Interface

All text through `aurora_i18n::tr!`, in English, French and Japanese. Shared
widgets, Palette colours and Tabler icons (rules § 3, `docs/BRANDING.md`).

### 9.1 Interactive viewport diagram (row 1.2)

In Preferences › Layout › Bars and windows › World viewport, above the
existing sliders (which stay as the keyboard route).

- Draws, to scale: the monitors around the window (numbered, with size and
  scale when not 100 %), the viewer window, the world rectangle, the margins,
  and the bars' area.
- **Drag** an edge of the world to move that margin; edges **snap to monitor
  borders** within a few diagram pixels; Escape during a drag puts it back.
  A drag cannot leave the world under a quarter of the window (the sliders
  still can, and the diagram then says the world fills the window).
- **Click** a monitor to select it; *Put the world on this monitor* (the
  existing "Fit to a monitor" math); double-click does the same.
- Works with unequal monitors, monitors above or left of the main one
  (negative desktop coordinates) and partly covered monitors. On Wayland the
  window and world are drawn without monitors, and monitor actions are
  disabled with the existing "no monitors" text.
- The same widget, read-only, previews a saved layout in the switcher.
- `publish_monitors` gains each monitor's scale (our own file).

### 9.2 Preferences: Layout › Layouts & workspaces (new tab)

- **Layouts**: list with the active one marked, "• modified", a lock icon
  and a favorite star. Apply, Update, Save as new…, Rename, Delete (bin, as
  for graphics profiles), Move up / down. For the selected layout: its parts,
  *Lock the window size*, and the window mode (Normal / Full screen /
  Borderless over monitors, the monitors picked on the diagram).
- **Toolbar toggle**: two layout pickers ("2 monitors" ⇄ "1 monitor").
- **Workspaces**: same list; per workspace: *Use layout* (Keep current or a
  name), the windows it opens, *Remember Inventory folders*.
- **Behaviour**: *When monitors change*: Ask / Apply the matching layout /
  Do nothing. *Reset bars and world*: viewport off, bars inside (windows
  untouched). *Previous arrangement*.
- Bars: the existing *bars inside the world* switch becomes a three-way
  choice: **Inside the world** (today's default) / **Around the world** —
  each bar goes in the margin next to the world when that margin has room
  for it, inside otherwise (the Folderstorm (C++) rule) / **Over the whole
  window**. Existing settings read the old switch.
- Help texts carry the search words "dual monitor", "layout", "workspace"
  (row 1.9).

### 9.3 Switcher

A floater, *Layouts & workspaces*.

- Search field focused on opening; type to filter; ↑ ↓ to select; **Enter
  applies**; Esc closes.
- Sections: Favorites, Workspaces, Layouts. Active entry marked; modified
  dot; lock icon.
- Footer: Apply · Update · Save new… (name and parts, inline) · Previous ·
  Preferences.
- Preview of the selected entry with the diagram widget: window, monitors,
  world, window rectangles, and warnings ("1 monitor missing").
- Never captured in a workspace, never moved by one.

### 9.4 Menu, toolbar, keyboard

- **World › Layouts**: favorites, *Layouts & workspaces…*, *Previous
  arrangement*, and *Unlock the viewer window* when locked.
- **Toolbar commands** (Toolbar buttons window): `workspaces` opens the
  switcher; `layout_toggle` flips between the two chosen layouts, its tooltip
  naming the one it will apply. Disabled, with a hint, until two layouts are
  chosen.
- **Keyboard** (Controls › Shortcuts): *Layouts & workspaces*, *Toggle
  layouts*, *Previous arrangement*, *Next favorite*, *Previous favorite*.
  **No default keys**, as in Folderstorm (C++): the user picks them, so no
  existing shortcut is taken. Ignored while typing, by Aurora's keybind rules.

## 10. Upstream hooks

Every line below is marked `// Folderstorm:`. "CP" is the checkpoint
(section 13).

| File | Hook | CP |
|---|---|---|
| `crates/aurora-viewer/src/ui/widgets.rs`, `Floater::show` | The shrink-to-fit room is the whole window while the world viewport is on (opening sizes and positions unchanged) | 1 |
| `crates/aurora-viewer/src/app.rs` | `mod folderstorm_workspaces;` beside `mod folderstorm_viewport;` | 3 |
| `crates/aurora-viewer/src/app.rs`, frame | One call after `publish_monitors`, in the existing Folderstorm block (apply steps, lock, monitor changes) | 3 |
| `crates/aurora-viewer/src/settings.rs` | Field `folderstorm_workspaces`, its default, and its line in `reset_keeping_account` (3 lines) | 3 |
| `crates/aurora-viewer/src/ui/options/pages.rs` | Tab label of the Layout page | 3 |
| `crates/aurora-viewer/src/ui/options/pages/layout.rs` | Match arm of the new tab | 3 |
| `crates/aurora-viewer/src/ui/widgets.rs`, `bare_window` | Read the same placement request as floaters, so the mini-map can be placed (2–3 lines) | 3 |
| `crates/aurora-viewer/src/ui/bars/side.rs` (3 existing hook lines), `app.rs` (existing `bars_rect` block) | Same lines, new "around the world" placement | 3 |
| `crates/aurora-viewer/src/ui/bars.rs`, World menu | One call drawing our *Layouts* submenu | 4 |
| `crates/aurora-viewer/src/app.rs`, interface pass | One call showing the card when the monitors change (4) and the switcher (5) | 4 |
| `crates/aurora-viewer/src/ui/toolbar/catalogue.rs` | Two commands | 5 |
| `crates/aurora-viewer/src/ui/bars.rs`, `run_command` | Two match arms → a request to our module | 5 |
| `crates/aurora-viewer/src/keybinds.rs` | One `Action` variant carrying our own action enum, its label arm, its line in the Controls list (no default chord) | 5 |
| `crates/aurora-viewer/src/app.rs`, keybind dispatch | One match arm | 5 |
| `crates/aurora-viewer/src/ui/inventory.rs`, `open_folder_window` | Return the new window's id (so a folder window can be placed) | 6 |
| `crates/aurora-i18n/locales/{en,fr,ja}.ftl` | Our Folderstorm section | all |
| `docs/TESTING.md` | Demo scenario rows | all |

New files (ours): `folderstorm/workspaces/{mod,store,fit,monitors,capture,apply,switcher,menu}.rs`,
`folderstorm/diagram.rs`, `app/folderstorm_workspaces.rs`,
`ui/options/pages/folderstorm_workspaces.rs`.

Candidate to send upstream once validated here (the human decides): making
`move_floater` work for bare windows.

## 11. Demo scenarios

All in the demo's own settings; nothing reaches the user's.

| Variable | What it shows | CP |
|---|---|---|
| `AURORA_DEMO_MONITORS="WxH[+X+Y][@scale],…[,window+X+Y]"` or `wayland` (syntax in `docs/TESTING.md`) | Pretend monitors (unequal, negative coordinates, a missing one) for the diagram and monitor matching, without the hardware | 2 |
| `AURORA_DEMO_OPTIONS=layout/0:cogs` | Opens the World viewport group with the diagram (existing mechanism) | 2 |
| `AURORA_DEMO_LAYOUTS=1` | Seeds "Wide" (world on the right half, Inventory and map on the left) and "Single" (viewport off, windows on one screen); opens the Layouts tab. Try with `AURORA_WINDOW_SIZE=3840,1080` | 3 |
| `AURORA_DEMO_LAYOUTS=toggle` | Flips between them every 5 s, for captures; `=lock` locks "Wide" | 4 |
| `AURORA_DEMO_SWITCHER=1` | Opens the switcher on the seeded entries | 5 |
| `AURORA_DEMO_WORKSPACES=1` | Seeds "Sorting" and "Exploring" on the demo inventory; `=folders` adds folder windows | 6 |

## 12. Tests

Pure unit tests (no GPU), in our modules:

- **Store**: serde round trip; unknown fields and window ids ignored; limits
  (names, counts, sizes, non-finite numbers); newer schema read-only; old
  `bars_in_viewport` read into the new choice.
- **Fit**: same frame → exact; smaller and larger frames keep the nearest
  anchor's offset; size cut to the room without touching the record; title
  bar reachable; DPI conversion of positions; minimized kept.
- **Monitors**: setup matching ignores order; a missing monitor; borderless
  rectangle = union of chosen monitors; title-bar-on-a-monitor test with
  negative coordinates; `spans_monitors` true for a 2-monitor stretched
  window, false for an old maximized-as-normal window on one monitor.
- **Diagram**: desktop ↔ diagram transform round trip; edge drag → margins,
  clamped at a quarter; snapping to monitor borders; unequal and offset
  monitors; no monitors.
- **Apply steps**: each step waits for the previous one; timeout; a new
  switch cancels the old; Previous swaps; RLVa-blocked windows skipped.

egui tests (an `egui::Context`, no GPU, like the tests in `ui/layout.rs`):

- capture → apply → same rectangles; a closed window takes its place when it
  opens; the minimized state kept; the mini-map honours a placement;
- checkpoint 1: a window narrower than the viewer window but wider than the
  world is no longer shrunk while the viewport is on;
- our read of the minimized state and expanded size agrees with
  `place_floater` (catches a renamed key upstream).

Plus the existing catalogue checks for the new i18n keys, and the checks of
section 4 of the rules (`fmt`, `clippy -D warnings`, `cargo test --workspace`).
Native checks on Windows by the human are listed per checkpoint.

## 13. Checkpoints

Each is one branch and one PR from an up-to-date `main`, with tests, a demo
scenario and the human's validation.

### Checkpoint 1: windows in the margins keep their size

- Floaters are no longer shrunk to the world while the viewport is on; they
  are kept inside the whole viewer window. The start-up 80 % rule is left
  as it is (decision 6).
- **Human checks:** with the world on the right monitor, put Inventory in the
  left margin; shrink the window to one monitor and back: windows keep their
  sizes (unless wider than the whole window).

### Checkpoint 2: interactive viewport diagram (row 1.2)

- The diagram of section 9.1, `AURORA_DEMO_MONITORS`. No new upstream hook.
- **Human checks:** unequal monitors, a monitor left of the main one, mixed
  scales; dragging and snapping; *Put the world on this monitor*.

### Checkpoint 3: layout profiles (row 1.3)

- Store, validation, capture and apply of the *World* and *Window positions*
  parts; the "around the world" bar placement; the Preferences tab; Previous;
  modified marker; the mini-map hook. No change to the viewer window yet.
- **Human checks:** save "Wide" and "Narrow" on the same window, switch:
  bars, world and windows move; a squashed window gets its size back.

#### Checkpoint 3 in detail (2026-11-13)

Written before the build, from `main` at `68d966f`. What differs from the
sketches of sections 7 to 10 is said here; "As built" below is filled in
when the checkpoint is done.

**Scope.** Layouts with two parts, *World viewport and bars* and *Window
positions*. Not in this checkpoint: the viewer window part and locking (4),
the monitor-change notice (4), the World menu (4), the switcher, the toolbar
toggle, favorites and shortcuts (5), workspaces (6). The store only holds
what this checkpoint uses; later checkpoints add fields, and a field this
build does not know is kept on a layout when it is saved again.

**1. Bars: "around the world".**

- The settings keep `bars_in_viewport` and gain `bars_around` (both in
  `ViewportSettings`, so in `folderstorm_viewport` of `settings.json`):
  *Inside the world* = `true, false` (today's default), *Around the world*
  = `true, true`, *Over the whole window* = `false, –`. No migration: an
  older file has no `bars_around` and reads as before. A three-way enum in
  the file would have changed every reader of `bars_in_viewport` (the
  session host's docked bar, the diagram) and the lines PR #41 edits.
- The rule is pure (`folderstorm/viewport/bars.rs`): the bars' area is still
  **one rectangle**, the world grown on each side by that side's bars when
  the margin between the world and the window has room for them; a side
  without room keeps its bars inside (the Folderstorm (C++) rule). So no
  change to `ui/bars/side.rs`: its three hook lines already lay the bars out
  in that rectangle.
- A side's bars are as thick as the last frame left them: the bars' area
  then, less the room it left for the windows (`Interface::last_room`).
  Measured, not guessed, so it follows the islands or the Legacy bars, the
  favourites row, a side toolbar, the interface scale.
- With the session host's bar docked over the world, the top side stays
  inside (the viewer's top bars under the host's bar, as today); the other
  three sides follow the rule.
- Hook: the existing `bars_rect` call in `app.rs` becomes one call into
  `app/folderstorm_workspaces.rs`.

**2. Store** (`folderstorm/workspaces/store.rs`), `settings.json` field
`folderstorm_workspaces`:

```json
{
  "layouts": [
    {
      "schema": 1,
      "name": "Wide",
      "world": { "enabled": true, "left": 1920, "right": 0, "top": 0, "bottom": 0,
                 "in_mouselook": true, "bars_in_viewport": true, "bars_around": true },
      "windows": {
        "frame": { "width": 3840.0, "height": 1080.0, "pixels_per_point": 1.0 },
        "windows": { "inventory": { "x": 8.0, "y": 48.0, "width": 900.0, "height": 700.0, "minimized": false } }
      },
      "monitors": [ { "x": 0, "y": 0, "width": 1920, "height": 1080, "scale": 1.0 } ]
    }
  ],
  "active_layout": "Wide",
  "modified": false,
  "pending": []
}
```

- `world` is the row 1.1 struct as it is (it now holds the bars' place), so
  section 7.1's separate `BarsPlace` is not needed. `world` and `windows`
  are each optional: an absent part is left alone when the layout is
  applied. `monitors` is recorded now for checkpoint 4's matching.
- Reading never fails and never drops: each entry of `layouts` is read on
  its own. One from a newer schema, or one that fails validation (section
  7.4), is kept exactly as read, listed as "made by a newer version" or
  "cannot be used", never applied and written back unchanged. Unknown
  window ids inside a valid layout are ignored when applying and kept in
  the file.
- Limits: 32 layouts, 64 windows a layout, names of 1 to 40 characters
  without control characters and unique ignoring case, sizes above zero and
  at most 16 384 points, every number finite.
- Kept by "Reset everything" like the graphics profiles. An absent field
  (every existing `settings.json`) is an empty store: nothing to migrate.

**3. What a layout captures, and what it never does.** Per allowlisted
window (`folderstorm/workspaces/windows.rs`, section 7.3): its position,
size and whether it is minimized. Nothing else: no open/closed state (a
layout never opens or closes a window), no contents, messages,
conversations, Inventory folders, filters or selections. The capture reads
egui's rectangle for the window id and Aurora's "minimized" and "expanded
size" entries for it, and nothing from the world state: there is no path
from a message or an Inventory item to the store, and a test checks the
saved JSON holds only the fields above.

**4. Fit** (`fit.rs`, pure). Saved positions are relative to the frame they
were saved in (the interface's size in points, and its pixels per point).
Same frame: exact. Another frame: the window keeps the offset from its
nearest of Aurora's eight anchors (same order of preference as
`ui/layout/anchor.rs`), its size is cut to the room, its title bar is kept
in the room. Another pixels-per-point: positions go through physical
pixels, sizes stay in points (section 8.6). The stored layout is never
rewritten by a fit.

**5. Apply** (`apply.rs`), without the viewer-window steps of section 8.2:

1. keep the current arrangement as *Previous* (both parts, memory only);
2. write the world part to the settings (takes effect on the next frame);
3. on the next frame, before the interface pass, place every window of the
   layout: `place_floater` for floaters (then `minimize_floater` if it was
   minimized), our own request for the mini-map (`bare_window` hook). A
   closed window takes its place when it next opens; its id stays in
   `pending` until then, so a restart issues the request again;
4. mark the layout active and not modified, save the settings.

A new apply replaces one still waiting for its frame. *Previous
arrangement* applies the kept arrangement the same way and keeps the one it
replaces, so pressing it twice comes back.

**6. Modified.** The world part is compared exactly with the settings. The
windows part is compared with a session baseline taken once an apply has
settled (and at start, after an Update, and when the interface changes
size): what Aurora really gave each window, so a window Aurora had to fit
or a floater with its own minimum size does not show as modified. A window
that then differs by more than 2 points, or changes its minimized state,
marks the layout modified; the mark is saved (`modified`) and cleared by
Apply or Update. Checked twice a second, never while a pointer button is
down.

**7. Preferences › Layout › Layouts** (new fourth tab,
`ui/options/pages/folderstorm_workspaces.rs`): the list (active mark,
"modified", unusable entries greyed with their reason); for the selected
layout Apply, Update (asks first), Rename, Delete (asks first), Move up and
down, and what it holds; *Save as new…* with a name and the two parts;
*Previous arrangement*; *Reset bars and world* (viewport off, bars inside;
windows untouched). The bars' three-way choice replaces the switch in the
World viewport group of the first tab. Search words: layout, dual monitor.

**8. Demo.** `AURORA_DEMO_LAYOUTS=1` seeds "Wide" and "Single" in the
demo's own settings and opens the tab; `=apply-wide` and `=apply-single`
also apply one at frame 120, for captures.

**9. Upstream hooks** (each marked `// Folderstorm:`): `app.rs` (the `mod`
line, one call after `publish_monitors`, the existing `bars_rect` call),
`settings.rs` (field, default, `reset_keeping_account`),
`ui/options/pages.rs` (`mod` line, tab label),
`ui/options/pages/layout.rs` (one match arm), `ui/widgets.rs`
`bare_window` (the placement request), the three catalogues,
`docs/TESTING.md`.

#### Checkpoint 3 as built (branch `claude/workspaces-cp3`, 2026-11-13)

Built as detailed above. What the build added or changed on the way:

- **Windows are asked twice when they change size and place at once.**
  Aurora keeps a window inside the interface with the size it had *before*
  a placement request (`Floater::show` constrains with egui's last size,
  and a window never shown is first measured at its default size). A window
  made smaller and moved towards an edge stopped short of its place. So
  `apply.rs` remembers where each window was asked to go; one that took its
  request and is not there is asked for its position again (`move_floater`,
  at most twice). No upstream change.
- **Nothing is placed before egui's first frame**: until then egui's screen
  is a made-up 10 000-point square, and a place fitted to it lands far away
  (`capture::room`).
- **The modified mark** is as designed: the world part compared exactly,
  the windows compared with a baseline taken 12 frames after an apply, a
  start, an Update or a change of the interface's size, and again when a
  waiting window is opened. Looked at every 30 frames, never while a
  pointer button is down.
- **Rename** gives the field the keyboard with the name selected. **Update**
  and **Delete** ask in the page (no modal dialog). A double click on a
  line applies it.
- **The store keeps `modified`** (the mark survives a restart) beside
  `layouts`, `active_layout` and `pending`.
- **The allowlist** (`windows.rs`): Inventory, the additional Inventory
  window, Conversations, People, Contacts, Places, Search, mini-map, world
  map, Performance, Controls and the Legacy camera and movement windows,
  Gestures, Appearance, AO, region tracker, area search, quick preferences,
  Destinations, money tracker. Ten of them are string ids; a test reads
  each back from the file that makes its window.

**Settings format.** As in "2. Store" above, plus `"modified": false`.
`folderstorm_viewport` gains `"bars_around": false`. Nothing to migrate: a
file without `folderstorm_workspaces` has no layouts, and a file without
`bars_around` reads its `bars_in_viewport` as before (tests:
`settings_without_the_field_have_an_empty_store`,
`settings_from_before_the_choice_read_as_they_did`).

**Upstream hooks** (line numbers at `c417ba4`):

| File | Line | Hook |
|---|---|---|
| `crates/aurora-viewer/src/app.rs` | 67 | `mod folderstorm_workspaces;` |
| `crates/aurora-viewer/src/app.rs` | 9938 | `self.folderstorm_workspaces_frame();` after `publish_monitors` and the chat upload's call |
| `crates/aurora-viewer/src/app.rs` | 16924–16925 | the existing `bars_rect` call (6 lines) is now `self.folderstorm_bars_rect(…)` |
| `crates/aurora-viewer/src/settings.rs` | 521–522, 2044, 3313 | the field, its default, kept by `reset_keeping_account` |
| `crates/aurora-viewer/src/ui/widgets.rs` | 1073–1076 | `bare_window` takes a layout's placement request |
| `crates/aurora-viewer/src/ui/options/pages.rs` | 17, 89 | `mod` line, the tab's label |
| `crates/aurora-viewer/src/ui/options/pages/layout.rs` | 188 | the tab's match arm |
| `crates/aurora-i18n/locales/{en,fr,ja}.ftl` | | 3 new `folderstorm-viewport-bars-*` keys, 2 reworded, 49 `fs-layouts-*` keys in a section of their own |
| `docs/TESTING.md` | | one paragraph, one table row |

No hook in `ui/bars/side.rs` (the bars' area is still one rectangle) and
none in `ui/layout.rs`.

**Tests.** Pure: `viewport::bars` (7), `workspaces::store` (13),
`workspaces::fit` (8), `workspaces::windows` (3). Frame tests with Aurora's
own `Floater` and `bare_window` in an `egui::Context`:
`workspaces::tests` (16: capture and the allowlist, apply and back, the
mini-map, a closed window and a restart, minimized windows, our reads of
Aurora's entries, Previous, the modified mark, a resized viewer window, a
smaller interface, absent parts, a replaced apply, unknown windows, Reset
bars and world, a tiny interface). The real Preferences window on the tab
(10, in `ui/options/pages/folderstorm_workspaces.rs`), one of which draws
it with the software painter (`FOLDERSTORM_TEST_CAPTURE_DIR`).

**Not checked here** (no graphics device in the sandbox): the bars drawn
around the world in the viewer itself, with the islands and with the Legacy
bars; the demo scenario; anything on Windows. The human's list is in the
checkpoint's report.

**Known limits.**

- The diagram of the world viewport still draws the bars' area as the
  world for *Around the world* (it does not know how thick the bars are).
- A window opened for the first time after an apply is not watched for the
  modified mark until the next baseline.
- Windows glued to each other (snapped side by side) are placed one by one;
  their link is Aurora's and is left as it is.
- With the session host's bar docked over the world, the top bars stay
  inside the world under it even with *Around the world*.

### Checkpoint 4: the viewer window in a layout, and locking

- Window frame modes (Normal, Full screen, Borderless over monitors), lock
  and automatic unlock, monitor-change notice, World › Layouts submenu.
- **Human checks:** "2 monitors" ⇄ "1 monitor" with both monitors present;
  unplug one while locked (unlocks, notice offers "1 monitor"); Aero Snap
  and Win+arrows on a locked window; borderless over two monitors; restart
  while locked.

#### Checkpoint 4 in detail (2026-11-13)

Written before the build, from `claude/workspaces-cp3` at `2f93583` (PR #48,
not merged: this checkpoint is stacked on it). What differs from the
sketches of sections 7 to 10 is said here; "As built" below is filled in
when the checkpoint is done.

**Scope.** The third part of a layout, the *viewer window*, in its three
modes, with the lock; the automatic unlock; the notice when the monitors
change; World › Layouts. And two of checkpoint 3's known limits: the
diagram's bars for *Around the world*, and a window first opened after an
apply being watched for the "modified" mark. Not here: the switcher, the
toolbar toggle, favorites and shortcuts (5), workspaces (6).

**1. Store.** A layout gains `window`, absent on every layout saved so far
(the viewer window is then left alone, as today):

```json
"window": { "mode": "normal", "x": -8, "y": 0, "width": 3840, "height": 1040,
            "maximized": false, "locked": true }
"window": { "mode": "fullscreen", "monitor": 0 }
"window": { "mode": "borderless", "monitors": [0, 1], "locked": true }
```

- Desktop pixels. `x`, `y` are the window's outer corner as the system
  gives it (absent where it does not: Wayland), `width`, `height` its
  inside: the pair winit reads and sets, so a save and an apply agree
  whatever the invisible borders of Windows are.
- `monitor` and `monitors` are places in the layout's own `monitors` list.
- The store gains `window_held` (the active layout's lock or borderless
  mode is in force: what a restart puts back) and `on_monitor_change`
  (`"ask"`, the default, `"apply"` or `"nothing"`; an unknown word reads as
  `"ask"`).
- Limits: the frame within ±32 768 pixels, sizes of 1 to 32 768, monitor
  places inside the list, no place twice. A layout that breaks one is kept
  as read and never applied, like the others. A file written here and read
  by checkpoint 3's build keeps `window` untouched (`Layout::other`).

**2. Monitors** (`monitors.rs`, pure): two setups are the same when they
hold the same monitors (place, size, scale) in any order; which of a
layout's monitors are present; the rectangle covering chosen monitors; the
start-up test of `App::resumed` (the title bar's corner on a monitor); a
watch that reports a new setup once it has not changed for 2 s; and the
rules of section 8.5, as one function from (saved frame, saved monitors,
monitors of now, window of now) to what is asked of the window:

| Saved | Asked of the window |
|---|---|
| Normal | Position only if its title bar lands on a monitor of now, else left where it is; size cut to the rectangle covering the monitors of now, and never under the viewer's minimum (800 × 560 points); maximized as saved. Locked only if every monitor of the layout is present |
| Full screen on monitor N | Aurora's full screen on N; N missing: on the monitor the window is on |
| Borderless over monitors | No border, over the chosen monitors still present; none present: left alone. Locked only if every monitor of the layout is present |
| any, where the system gives no monitor list | Normal: the size only. Full screen: where the window is. Borderless: left alone |

**3. The window, step by step** (`window.rs`, pure). The module never
touches winit. Each frame the app tells it what the window is like
(`Seen`: inside size, outer corner, scale, maximized, full screen,
minimized, border, resizable, and which window it is) and runs the
commands it gets back (`Command`: full screen on or off, maximized, border,
resizable, position, size). Applying a frame is a small machine fed by
`Seen`:

1. unlock; leave full screen if the goal is not full screen (through
   Aurora's own setting, so its saved "windowed" geometry stays right) and
   wait until the window says so;
2. leave maximized, set the border, wait until the window says so (or
   0.5 s);
3. position and size; wait until the window reports the size asked, or 10
   frames without change, or 1.5 s (section 8.2);
4. maximize or enter full screen if the goal says so; lock.

Only then are the layout's windows placed (they are fitted to the
interface of that moment, and the report's "fitted" count is taken then).
A new apply replaces the machine. No viewer window to drive (a capture, a
forced size, no window yet): the window part is skipped and the rest
applies as in checkpoint 3.

**4. Lock** (`viewer.rs`). What is wanted of the window comes from the
store alone: `window_held` and the active layout's frame give *locked*
(not resizable) and *borderless* (no border), or neither. The keeper
compares that with what it last asked and sends the difference, so:

- **Unlock** (World › Layouts, Preferences) clears `window_held`: border
  and resizing come back at once; the window keeps its place and size.
- Applying a layout without a lock, or without a window part, unlocks.
- Entering Aurora's full screen unlocks.
- A monitor setup that has lost one of the layout's monitors unlocks (once
  settled, 2 s).
- A window made anew (GPU recovery) is locked again; at start a held
  layout is locked again when its monitors are all present, a borderless
  one put back over them, and otherwise `window_held` is cleared. All of it
  once the world shows, like the rest of the layouts (section 8.7).
- Nothing is ever sent to a window no layout holds: without layouts the
  viewer window behaves exactly as Aurora's.

**5. Save, Update, Previous, modified.**

- *Save as a new layout* gains the part *Viewer window* and the box *Lock
  the window size*. It records the window of now (section 8.1): Aurora's
  full screen → full screen on its monitor; no border and exactly over
  monitors → borderless over them; else normal (maximized: with the normal
  geometry Aurora remembers for it). Ticking the lock locks at once.
- *Update* reads the window again for a layout that has the part and keeps
  that layout's lock box.
- *Previous arrangement* brings the window back too, when the apply it
  undoes moved it; the lock in force then comes back with it.
- *Modified*: the window is compared with what it was once the apply had
  settled (as the windows are): more than 4 pixels of position or size, or
  another maximized or full-screen state, marks the layout.
- The selected layout's window can be edited in the tab: mode (Normal /
  Full screen / Borderless over monitors), the monitors on a small picture
  of the layout's monitors (click to choose), the lock box. Editing the
  layout in use marks it modified; *Apply* then does it.

**6. When the monitors change** (`viewer.rs`, section 8.5). The setup is
read from what `publish_monitors` already publishes each second. Once a
new setup has held for 2 s: the automatic unlock above, then by
`on_monitor_change`:

- *Ask*: a card at the top of the world, "Your monitors changed. Use the
  layout “1 monitor”?" [Apply] [Not now], for the first layout of the list
  saved on exactly this setup, unless the layout in use is already one.
  No such layout and the window was just unlocked: the card says so, [OK].
- *Apply the matching layout*: applied at once; the card says which, with
  [Go back] (Previous arrangement) and [OK].
- *Do nothing*: no card.

The card is not modal, never takes the keyboard, and goes away when a
layout is applied or the monitors change again. The first setup seen in a
session is only noted: nothing is offered at start (section 8.3).

**7. World › Layouts** (`menu.rs`): the usable layouts (a click applies;
the one in use ticked), *Layouts…* (Preferences › Layout › Layouts until
the switcher of checkpoint 5), *Previous arrangement*, and *Unlock the
viewer window* while it is held. The menu has no access to the settings:
it reads a small view the frame call publishes when the list changes, and
leaves requests the next frame call runs.

**8. Preferences.** The tab gains: the *Viewer window* part and the lock
box in *Save as a new layout*; a lock on the lines of locked layouts; the
window editor of point 5; under *Arrangement*, *Unlock the viewer window*
and *When the monitors change* (Ask / Apply the matching layout / Do
nothing).

**9. Checkpoint 3's limits.**

- The diagram draws the bars where the settings put them: for *Around the
  world*, on the area `viewport::bars::around` gives with the bars'
  thickness as last measured, so a side with room shows its bars in the
  margin and a side without shows them inside; as thick as they are, to
  scale (at least 2 points of the diagram).
- A window of the layout the baseline did not know (never shown before in
  this session) joins the baseline the first time it is seen, and is
  watched from then on.

**10. Demo.** `AURORA_DEMO_LAYOUTS=toggle` applies "Wide" and "Single" in
turn every 5 s; `=lock` gives "Wide" a locked viewer window and applies
it; `=unplug`, with two pretend monitors in `AURORA_DEMO_MONITORS`, takes
the last one away after 10 s (the card offers "Single"). The viewer-window
part needs a real window: not under `AURORA_WINDOW_SIZE` or a capture.

**11. Upstream hooks** (each marked `// Folderstorm:`), beside checkpoint
3's: `ui/bars.rs`, World menu: one call (section 10); `app.rs`, interface
pass: one call drawing the card, beside the session host's notice. This is
section 10's "one call showing the switcher", brought forward: checkpoint
5 draws the switcher from the same call. The three catalogues;
`docs/TESTING.md`.

**12. Tests.** Pure: the store's new fields and limits, a file from
checkpoint 3 read unchanged; `monitors` (same setup in any order, missing,
covering rectangle, title bar with negative coordinates, the watch's 2 s,
each row of the table above); `window` (the machine against a pretend
window that answers at once, late, or never: every wait and every
timeout; a replaced apply). Frame tests with a pretend viewer window:
save → apply → same frame; the windows placed only once the window has
its size; lock, unlock, the lock after a "restart" and after a new
window; full screen unlocks; a lost monitor unlocks and offers the
matching layout; the three policies; Previous; the modified mark of the
window; a newly shown window watched. The tab, the card and the menu
drawn with the software painter.

#### Checkpoint 4 as built (branch `claude/workspaces-cp4`, 2026-11-13)

Built as detailed above, stacked on checkpoint 3 (PR #48). What the build
added or changed on the way:

- **Ten still frames are not the end while the window has not moved at
  all.** Section 8.2 waits for "the requested size, 10 frames without
  change, or 1.5 s". A window that has not heard the request yet looks the
  same as one that refuses it, so the ten still frames count once the
  window has changed at all since the request (the system cut the size), or
  after half a second without any change. The 1.5 s limit is as planned.
- **The size is always cut to the monitors there are**, not only when the
  window stays where it is: a two-monitor layout applied by mistake on one
  monitor does not hang half its window, and the world with it, off the
  screen. The saved layout keeps its values.
- **A layout that holds the window puts it back at start** when it is not
  where the layout has it (within 4 pixels), then locks it: Aurora reopens
  a stretched window at 80 % of one monitor, and a borderless one with its
  border. Section 8.3's "nothing is re-applied at start" still holds for
  the world, the bars, the windows and every layout that holds nothing. A
  window already in place is only locked.
- **The lock box of the layout in use applies at once** in the tab's
  editor (a normal window's; a borderless one's while it is held). Another
  mode or other monitors mark the layout modified until *Apply*.
- **Previous arrangement** brings the viewer window back only when the
  apply it undoes had a viewer-window part: a layout without one never
  moves the window, in either direction.
- **A card that only tells** (a layout applied by your choice, the window
  unlocked) goes away after 12 s; one that asks waits for its answer.
- **The first look at the monitors** is 30 frames after the world shows,
  and they are looked at twice a second (they are published once a
  second): the card comes 2 to 3.5 s after a change.
- **The diagram draws the bars to scale** in all three places, from the
  thickness last measured (`viewport::bars::last_thickness`); before any
  measure, a top and a bottom bar of a nominal thickness.

**Settings format.** A layout may hold `"window"` as in "1. Store" above.
The store may hold `"window_held": true` and `"on_monitor_change":
"apply"` or `"nothing"`; both are left out at their defaults, so a store
that uses none of this is written exactly as checkpoint 3 wrote it. Nothing
to migrate, and checkpoint 3's build keeps what it does not know (tests:
`a_layout_from_before_the_viewer_window_reads_and_writes_as_it_was`,
`the_viewer_window_is_saved_in_its_three_modes`,
`a_viewer_window_out_of_its_limits_makes_the_layout_unusable`,
`the_hold_and_the_monitors_choice_survive_a_restart`).

**Upstream hooks** (line numbers at the checkpoint's last commit; all
marked `// Folderstorm:`):

| File | Line | Hook |
|---|---|---|
| `crates/aurora-viewer/src/app.rs` | 9938 | checkpoint 3's call now passes the window: `self.folderstorm_workspaces_frame(&gfx.window);` (the frame function holds the window apart from `self`) |
| `crates/aurora-viewer/src/app.rs` | 16380 | new: `self.folderstorm_workspaces_notice(&ctx, &p);` in the interface's pass, after the session host's notice |
| `crates/aurora-viewer/src/ui/bars.rs` | 1153 | new: `crate::folderstorm::workspaces::menu::show(ui, p);` in the World menu, after *Owned land* |
| `crates/aurora-i18n/locales/{en,fr,ja}.ftl` | | 37 new `fs-layouts-*` keys in our section, 4 reworded |
| `docs/TESTING.md` | | three paragraphs, one table row widened |

Nothing new in `settings.rs`, `ui/widgets.rs` or `ui/options/`: the store
is one field, and the tab is ours.

**New files:** `folderstorm/workspaces/{monitors,window,viewer,menu,notice}.rs`,
`folderstorm/workspaces/tests/viewer.rs`. The only code that touches winit
is `app/folderstorm_workspaces.rs` (`folderstorm_viewer_window`,
`folderstorm_viewer_command`): about sixty lines, not testable here.

**Tests.** Pure: `workspaces::store` (+5), `workspaces::monitors` (14),
`workspaces::window` (11, against a pretend window that answers at once,
late, or never), `workspaces::capture` (4), `diagram` (+2, one drawn).
Frame tests with the pretend window and pretend monitors,
`workspaces::tests::viewer` (27): save and apply, the windows placed only
once the window has its size, lock and unlock, a restart (in place, shrunk
by Aurora, a monitor gone), a window made anew, full screen, a lost monitor
with each of the three choices, the card's buttons, World › Layouts,
Previous, the modified mark, Update, borderless, full screen on a monitor,
no window to drive, the editor, a window first shown after the baseline.
The tab (+8). Drawn with the software painter
(`FOLDERSTORM_TEST_CAPTURE_DIR`): `preferences-layouts-window.png`,
`layouts-card-and-menu.png`, `diagram-bars-{inside,around,window}.png`.

**Not checked here** (no graphics device, no window system in the
sandbox): everything winit does when asked — `set_resizable(false)` and
Aero Snap, `set_decorations(false)` and the exact cover of the monitors,
positions with the invisible borders of Windows, how fast Windows answers,
what Windows does to the window when a monitor is unplugged, a GPU
recovery; the three demo modes; macOS and Linux. The human's list is in the
checkpoint's report.

**Known limits.**

- The viewer-window part and the lock work once the world shows, not on
  the login screen: a held window is free until then (section 8.7).
- An existing layout cannot gain or lose its viewer-window part: save a
  new layout with the parts wanted.
- A locked normal window can still be moved by its title bar (section
  8.4); the layout then shows as modified.
- Under the session host, bringing a character forward puts its window on
  the host's stage, whatever a layout asked: the layout then shows as
  modified.
- With pretend monitors (`AURORA_DEMO_MONITORS`), the real window is
  judged against monitors that are not there: the demo of the card is
  right, the window's place may not be.
- The card is drawn in mouselook too, where it cannot be clicked.
- Checkpoint 3's two other limits stand: windows glued to each other are
  placed one by one; with the session host's bar docked over the world,
  the top bars stay inside it.

### Checkpoint 5: switcher and quick toggle (row 1.5)

- Switcher with filter, Enter, favorites, Save new, Update, preview;
  toolbar commands `workspaces` and `layout_toggle`; keyboard actions.
- **Human checks:** the toggle button flips monitors in one click; shortcuts
  assigned in Controls work and do nothing while typing.

### Checkpoint 6: workspaces (row 1.4)

- Open / close allowlisted windows, additional Inventory window, folder
  windows with *Remember Inventory folders* (per account), Conversations
  geometry only, restore report.
- **Human checks:** "Sorting" and "Exploring" on both layouts; RLVa
  `@showinv` blocks Inventory; a deleted folder skips with a note; another
  account skips the folders.

## 14. Later (outline only)

- **1.6 Favorites strip.** Favorite layouts and workspaces after a divider on
  the islands' favourites row (`ui/bars/islands.rs`,
  `favorites_row_bottom`), overflow in *More…*, hidden with the bars and in
  mouselook. Reads `Store.favorites`; one hook where the row is drawn.
- **1.7 Workspace context.** Optional parts of a workspace: a graphics
  profile *by name* (Aurora's `graphics_saved`), a camera preset, and HUDs to
  add, stored as the user's own Inventory item ids (never asset ids) and worn
  through the normal outfit paths (outfit safety, RLVa), never detaching
  anything else. Missing-monitor recovery is already section 8.5. MCP tools
  `workspace_list` / `workspace_apply` belong with row 4.6.
- **1.8 Backgrounds.** An image painted in the margins outside the world, on
  egui's background layer, before the windows; no renderer change. Optional
  per layout.
- **1.9 Search keywords.** Help texts (section 9.2).
- Startup restore of a workspace (opt-in), export / import, toolbar button
  sets per workspace: only if the human asks.

## 15. Risks

| Risk | Mitigation |
|---|---|
| Win32 window behaviour differs from winit's promise (invisible DWM borders shift outer positions by a few pixels; Windows moves windows itself on monitor changes) **(verify on Windows)** | Apply in steps and wait for the size to settle; compare with a tolerance; 2 s debounce on monitor changes |
| A locked or borderless window becomes unreachable after a monitor change | Automatic unlock; Unlock always in World › Layouts; lock not restored at start unless all monitors match |
| Upstream renames a `Panels` field or a window id | The allowlist uses the `Panels` fields and the windows' own `WINDOW` constants (`people::WINDOW`, `search::WINDOW`…) where they exist, so a rename fails to compile at the merge; the few string ids left are listed in one place |
| Placement requests for closed windows are memory-only | Re-issued at start for windows not shown since the switch |
| Hooks in `widgets.rs` change shared floater behaviour | Kept to the viewport case and bare windows; covered by egui tests; human validation |
| `settings.json` grows | Bounded: 32 + 32 entries, 64 windows each, a few tens of kB at most |

## 16. Decisions (the human, 2026-11-11)

1. **Split:** layouts hold window positions; workspaces hold which windows
   are open (section 6), instead of Folderstorm (C++)'s "layout = viewport
   and bars, workspace = full snapshot". **Yes.**
2. **Scope:** layouts and workspaces per profile, shared by the accounts
   logging in on that profile; only Inventory folder references per account.
   **Yes.**
3. **Window moves after a switch** are saved only by an explicit *Update*,
   with the "modified" marker (an "auto-remember" option can come later).
   **Yes.**
4. **Locking:** a per-layout *Lock the window size* box, plus an opt-in
   *Borderless over monitors* mode that also locks the position, Windows
   first. **Yes.**
5. **When the monitors change:** ask by default. **Yes.**
6. **Start-up rule:** tested on Windows with the combined build: a window
   stretched over two monitors reopens at 80 % of one monitor, the world then
   fills it (the viewport's quarter-of-the-window fallback), and stretching it
   back puts the windows where they were. The human likes this: the rule is
   kept, and no hook is added to `App::resumed`.

## 17. Verified in code vs inferred

**Verified by reading this repository:** the anchor and follow rules and that
clamping does not rewrite anchors (`ui/layout.rs`, `ui/layout/anchor.rs`);
the floater shrink, its recording and the per-frame `expanded_size` write
(`ui/widgets.rs`); the work area computed inside the world when the bars are
inside it (`app.rs`); the start-up 80 % rule and its test
(`App::resumed`); `WindowedGeometry` (logical size, physical position); the
800 × 560 minimum; `place_floater`, `move_floater`, `size_floater` and that
`bare_window` ignores placement requests; Preferences applying at once;
graphics profiles in `settings.json`, kept by "Reset everything"; `Panels`
flags; RLVa closing Inventory and maps each frame; the additional Inventory
window's fixed id and folder windows' random ids; `publish_monitors` every 60
frames and no position on Wayland; the toolbar catalogue, `run_command` and
the keybind `Action` structure; winit 0.30 and egui 0.37 in the tree.

**Seen on Windows by the human:** a window stretched over two monitors
reopens at 80 % of one; with the viewport on, the world then fills it and
stretching the window back restores the windows' places.

**Inferred, to check on Windows:** that the squashed
windows the human saw came from the sliver work area; `set_resizable(false)`
blocking Aero Snap; Win+Shift+arrows on a borderless window; invisible DWM
borders and outer vs inner positions; how Windows moves the viewer window
when a monitor is unplugged; DPI of a window spanning monitors with
different scales.
