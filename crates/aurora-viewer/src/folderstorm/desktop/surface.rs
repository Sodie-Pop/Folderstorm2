//! The desktops as drawn: one area per room, on the layer between the
//! margins' background and the windows, its shortcuts on the grid, their
//! hover, selection, drag between cells and menus, the Inventory folders
//! and the toolbar buttons dropped on it, the name edited in place and
//! the keyboard on a focused shortcut (F2, Delete, the arrows)
//! (folderstorm/plans/margin-desktop.md, "Drawing order and input",
//! "Arranging and editing", "Accessibility and keyboard").

use super::place::Grid;
use super::store::{Desktop, Error, MAX_NAME, MAX_SHORTCUTS, Size, Target};
use super::targets::{self, Face, Greyed};
use super::{Env, Request};
use crate::theme::Palette;
use crate::ui::icons::Icons;
use crate::ui::menu;
use aurora_i18n::tr;
use egui::{Pos2, Rect, Sense, Stroke, StrokeKind, vec2};
use uuid::Uuid;

/// A slot's room and its cells.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Room {
    pub slot: u8,
    pub rect: Rect,
    pub grid: Grid,
}

/// The area of the desktop of `slot`.
pub(super) fn area_id(slot: u8) -> egui::Id {
    egui::Id::unique("folderstorm_desktop_room").with(slot)
}

/// What the surface remembers between frames.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct State {
    /// The selected shortcut.
    pub selected: Option<u32>,
    /// The shortcut being dragged.
    drag: Option<u32>,
    /// The cell the desktop's menu was opened on.
    menu_cell: Option<(u8, (u16, u16))>,
    /// The shortcut whose name is being edited ([`rename_text`]).
    pub rename: Option<u32>,
    /// Its field was just opened: it takes the keyboard, its text selected.
    rename_fresh: bool,
}

fn state_id() -> egui::Id {
    egui::Id::unique("folderstorm_desktop_state")
}

pub(super) fn state(ctx: &egui::Context) -> State {
    ctx.data(|d| d.get_temp(state_id())).unwrap_or_default()
}

/// Selects the shortcut `id` (one just added from elsewhere).
pub fn select(ctx: &egui::Context, id: u32) {
    let mut state = state(ctx);
    state.selected = Some(id);
    ctx.data_mut(|d| d.insert_temp(state_id(), state));
}

fn rename_id() -> egui::Id {
    egui::Id::unique("folderstorm_desktop_rename")
}

/// The name being typed for [`State::rename`].
pub(super) fn rename_text(ctx: &egui::Context) -> String {
    ctx.data(|d| d.get_temp(rename_id())).unwrap_or_default()
}

/// The widget of the shortcut `id` in the desktop of `slot`.
pub(super) fn shortcut_id(slot: u8, id: u32) -> egui::Id {
    area_id(slot).with(("shortcut", id))
}

/// What the pointer and the menus asked this frame.
enum Op {
    Open(u32),
    /// Open a folder's window beside its shortcut.
    OpenFolder {
        folder: Uuid,
        cell: Rect,
        room: Rect,
    },
    ShowFolder(Uuid),
    /// Inventory dropped on a room: the folders it gives, or why none.
    DropFolders {
        slot: u8,
        cell: Option<(u16, u16)>,
        offer: Result<targets::Picked, targets::Refusal>,
    },
    Remove(u32),
    Add {
        command: String,
        slot: u8,
        cell: Option<(u16, u16)>,
    },
    /// The name typed for a shortcut; `own`: its target's own name (typed
    /// back, or nothing typed: the shortcut has no name of its own).
    Rename {
        id: u32,
        text: String,
        own: String,
    },
    /// An icon of [`super::icons::CHOICES`]; None: its target's own.
    Icon(u32, Option<&'static str>),
    /// To the first free cell of another monitor's desktop.
    MoveTo {
        id: u32,
        slot: u8,
    },
    /// *Arrange by name* in the desktop of that slot.
    Arrange(u8),
    ToolbarButtons,
    /// Say this in the chat's notices.
    Say(String),
    /// Put a shortcut in a cell; the one shown there takes its place.
    Drop {
        id: u32,
        slot: u8,
        cell: (u16, u16),
    },
    Size(Size),
    Settings,
}

/// Icons of the menus (Tabler).
const ICON_OPEN: &str = "external-link";
const ICON_REMOVE: &str = "trash-filled";
const ICON_ADD: &str = "plus";
const ICON_SIZE: &str = "dimensions";
const ICON_SETTINGS: &str = "settings-filled";
const ICON_SHOW: &str = "folder-open-filled";
const ICON_RENAME: &str = "pencil-filled";
const ICON_ICON: &str = "mood-smile-filled";
const ICON_DEFAULT: &str = "restore";
const ICON_MOVE: &str = "arrows-move";
const ICON_ARRANGE: &str = "sort-ascending-letters";
const ICON_TOOLBAR: &str = "layout-bottombar-filled";

/// The icons this module names, for the test that they are embedded.
#[cfg(test)]
pub(super) const ICONS: &[&str] = &[
    ICON_OPEN,
    ICON_REMOVE,
    ICON_ADD,
    ICON_SIZE,
    ICON_SETTINGS,
    ICON_SHOW,
    ICON_RENAME,
    ICON_ICON,
    ICON_DEFAULT,
    ICON_MOVE,
    ICON_ARRANGE,
    ICON_TOOLBAR,
    super::ICON_ADD_TO_DESKTOP,
];

/// What adding folders to a desktop did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Added {
    /// New shortcuts.
    pub added: usize,
    /// Folders that desktop has a shortcut to already.
    pub existing: usize,
    /// Folders left out: that desktop has no free cell.
    pub no_room: usize,
    /// Folders left out: the desktops hold [`MAX_SHORTCUTS`] already.
    pub limit: usize,
    /// The first shortcut added, else the first one found there already.
    pub first: Option<u32>,
}

/// Why a desktop takes no new shortcut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoRoom {
    /// Its room has no free cell.
    Full,
    /// The desktops hold [`MAX_SHORTCUTS`] already.
    Limit,
}

impl NoRoom {
    /// What the notice, and the greyed *Add a command ›*, say.
    pub fn text(self) -> String {
        match self {
            NoRoom::Full => tr!("fs-desktop-full").to_owned(),
            NoRoom::Limit => tr!("fs-desktop-limit", count = MAX_SHORTCUTS),
        }
    }
}

/// Why the room `grid`, with `taken` cells shown, takes no new shortcut
/// now; None when it does.
pub(super) fn no_room(desktop: &Desktop, grid: &Grid, taken: &[(u16, u16)]) -> Option<NoRoom> {
    if desktop.shortcuts.len() >= MAX_SHORTCUTS {
        Some(NoRoom::Limit)
    } else if grid.first_free(taken).is_none() {
        Some(NoRoom::Full)
    } else {
        None
    }
}

/// The shortcut that the desktop of `slot` has to `target` already.
fn existing(desktop: &Desktop, slot: u8, target: &Target) -> Option<u32> {
    desktop
        .shortcuts
        .iter()
        .find(|s| s.slot == slot && s.target == *target)
        .map(|s| s.id)
}

/// What adding a command to a room did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandAdded {
    New(u32),
    /// That desktop has it already: shown instead.
    Existing(u32),
    Refused(NoRoom),
    /// A command that never opens from a shortcut.
    Never(Greyed),
}

/// Adds a shortcut to `command` in `room`, whose shown cells are `taken`:
/// in `cell` when it is free, else in the first free cell.
pub(super) fn add_command(
    desktop: &mut Desktop,
    room: &Room,
    taken: &[(u16, u16)],
    command: &str,
    cell: Option<(u16, u16)>,
) -> CommandAdded {
    let target = Target::command(command);
    // (a full desktop still shows the one it has)
    if let Some(id) = existing(desktop, room.slot, &target) {
        return CommandAdded::Existing(id);
    }
    if let Some(why) = targets::refuses_adding(command) {
        return CommandAdded::Never(why);
    }
    let Some(cell) = cell.filter(|cell| !taken.contains(cell)).or_else(|| room.grid.first_free(taken)) else {
        return CommandAdded::Refused(NoRoom::Full);
    };
    match desktop.add(target, room.slot, cell) {
        Ok(id) => CommandAdded::New(id),
        Err(Error::Existing(id)) => CommandAdded::Existing(id),
        Err(Error::Full | Error::Missing) => CommandAdded::Refused(NoRoom::Limit),
    }
}

/// The cells the shortcuts of `room` are shown in.
fn shown_in(desktop: &Desktop, room: &Room) -> Vec<(u32, (u16, u16))> {
    let stored: Vec<_> = desktop
        .shortcuts
        .iter()
        .filter(|s| s.slot == room.slot)
        .map(|s| (s.id, s.cell()))
        .collect();
    super::place::shown(&stored, &room.grid)
}

fn taken_in(desktop: &Desktop, room: &Room) -> Vec<(u16, u16)> {
    shown_in(desktop, room).into_iter().map(|(_, cell)| cell).collect()
}

/// Adds a shortcut to `command` in the first free cell of the first
/// desktop (*Add to desktop* of a toolbar button's menu). Without a room
/// (the world fills the window) it is kept for when there is one.
pub fn add_command_first(desktop: &mut Desktop, rooms: &[Room], command: &str) -> CommandAdded {
    if let Some(room) = rooms.first() {
        let taken = taken_in(desktop, room);
        return add_command(desktop, room, &taken, command, None);
    }
    let target = Target::command(command);
    if let Some(id) = existing(desktop, 0, &target) {
        return CommandAdded::Existing(id);
    }
    if let Some(why) = targets::refuses_adding(command) {
        return CommandAdded::Never(why);
    }
    let free = (0..super::store::MAX_CELL)
        .flat_map(|col| (0..UNSEEN_ROWS).map(move |row| (col, row)))
        .find(|at| !desktop.shortcuts.iter().any(|s| s.slot == 0 && s.cell() == *at));
    let Some(free) = free else {
        return CommandAdded::Refused(NoRoom::Full);
    };
    match desktop.add(target, 0, free) {
        Ok(id) => CommandAdded::New(id),
        Err(Error::Existing(id)) => CommandAdded::Existing(id),
        Err(Error::Full | Error::Missing) => CommandAdded::Refused(NoRoom::Limit),
    }
}

/// What *Bring them here* did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Brought {
    /// Shortcuts moved to the first desktop.
    pub moved: usize,
    /// Shortcuts that desktop had already (to the same thing): the one of
    /// the monitor not in use went.
    pub merged: usize,
    /// Shortcuts left where they were: that desktop is full.
    pub left: usize,
}

/// The shortcuts that stand on a slot without a room go to the first free
/// cells of the first room. One whose target that desktop has a shortcut
/// to already is removed (a desktop never holds the same thing twice).
pub fn bring_here(desktop: &mut Desktop, rooms: &[Room]) -> Brought {
    let mut out = Brought::default();
    let Some(room) = rooms.first() else {
        return out;
    };
    let live: Vec<u8> = rooms.iter().map(|room| room.slot).collect();
    let away: Vec<_> = desktop
        .shortcuts
        .iter()
        .filter(|s| !live.contains(&s.slot))
        .map(|s| (s.id, s.target.clone()))
        .collect();
    for (id, target) in away {
        if existing(desktop, room.slot, &target).is_some() {
            out.merged += usize::from(desktop.remove(id).is_ok());
            continue;
        }
        match room.grid.first_free(&taken_in(desktop, room)) {
            Some(cell) if desktop.place(id, room.slot, cell) => out.moved += 1,
            _ => out.left += 1,
        }
    }
    out
}

/// The shortcuts of `slot` in the order of their names as `env` shows
/// them (so nothing RLVa hides is read), then of their ids.
fn by_name(desktop: &Desktop, env: &Env, slot: u8) -> Vec<u32> {
    let mut named: Vec<_> = desktop
        .shortcuts
        .iter()
        .filter(|s| s.slot == slot)
        .map(|s| (targets::face(s, &env.facts, &env.folders).name.to_lowercase(), s.id))
        .collect();
    named.sort();
    named.into_iter().map(|(_, id)| id).collect()
}

/// Without a room (the world fills the window), shortcuts are kept in
/// columns of this many cells; a room shows them where they fit.
const UNSEEN_ROWS: u16 = 8;

/// Adds a shortcut to each of `folders` in the desktop of
/// `slot` (None: the first room's): from `cell` when one is given, each in
/// the free cell nearest to it; else in the first free cells. Without a
/// room for that slot they are kept for when it has one.
pub fn add_folders(desktop: &mut Desktop, rooms: &[Room], folders: &[Uuid], slot: Option<u8>, cell: Option<(u16, u16)>) -> Added {
    let slot = slot.or(rooms.first().map(|room| room.slot)).unwrap_or(0);
    let grid = rooms.iter().find(|room| room.slot == slot).map(|room| room.grid);
    let mut out = Added::default();
    for folder in folders {
        let target = Target::Folder { folder: *folder };
        // (a full desktop still shows the one it has)
        if let Some(id) = existing(desktop, slot, &target) {
            out.existing += 1;
            out.first.get_or_insert(id);
            continue;
        }
        let stored: Vec<_> = desktop
            .shortcuts
            .iter()
            .filter(|s| s.slot == slot)
            .map(|s| (s.id, s.cell()))
            .collect();
        let free = match &grid {
            Some(grid) => {
                let taken: Vec<_> = super::place::shown(&stored, grid).into_iter().map(|(_, at)| at).collect();
                match cell {
                    Some(cell) => grid.nearest_free(cell, &taken),
                    None => grid.first_free(&taken),
                }
            }
            None => (0..super::store::MAX_CELL)
                .flat_map(|col| (0..UNSEEN_ROWS).map(move |row| (col, row)))
                .find(|at| !stored.iter().any(|(_, taken)| taken == at)),
        };
        let Some(free) = free else {
            out.no_room += 1;
            continue;
        };
        match desktop.add(target, slot, free) {
            Ok(id) => {
                out.added += 1;
                // (the new one, rather than one that was there)
                if out.added == 1 {
                    out.first = Some(id);
                }
            }
            Err(Error::Existing(id)) => {
                out.existing += 1;
                out.first.get_or_insert(id);
            }
            Err(Error::Full | Error::Missing) => {
                out.limit += 1;
            }
        }
    }
    out
}

/// The label of a size.
pub fn size_label(size: Size) -> &'static str {
    match size {
        Size::Small => tr!("fs-desktop-size-small"),
        Size::Medium => tr!("fs-desktop-size-medium"),
        Size::Large => tr!("fs-desktop-size-large"),
    }
}

/// Draws the desktops of `rooms` and applies what was asked to `desktop`.
pub(super) fn show(ctx: &egui::Context, desktop: &mut Desktop, env: &Env, rooms: &[Room]) -> Vec<Request> {
    let before = state(ctx);
    let mut state = before;
    // the cell each shortcut is shown in, room by room
    let shown: Vec<Vec<(u32, (u16, u16))>> = rooms
        .iter()
        .map(|room| {
            let stored: Vec<_> = desktop
                .shortcuts
                .iter()
                .filter(|s| s.slot == room.slot)
                .map(|s| (s.id, s.cell()))
                .collect();
            super::place::shown(&stored, &room.grid)
        })
        .collect();
    let pointer = ctx.input(|i| i.pointer.latest_pos());
    // Inventory being dragged (an item, a folder or a selection)
    let cargo = crate::ui::inventory::drag::carried_ids(ctx);
    // a toolbar button being dragged (from a toolbar or the Toolbar
    // buttons window)
    let button = super::toolbar_cargo(ctx);
    let mut ops = Vec::new();
    let mut dragged = None;
    // a shortcut that went while its name was edited
    if state.rename.is_some_and(|id| desktop.shortcut(id).is_none()) {
        state.rename = None;
    }
    // (read before the field: it takes the key)
    let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    // the keyboard asked for the desktop: its selected shortcut, else its first
    let focus = env.focus.then(|| {
        let all = || shown.iter().flatten().map(|(id, _)| *id);
        state.selected.filter(|id| all().any(|other| other == *id)).or_else(|| {
            rooms
                .first()
                .zip(shown.first())
                .and_then(|(room, cells)| room.grid.cells().find_map(|at| cells.iter().find(|(_, cell)| *cell == at)))
                .map(|(id, _)| *id)
        })
    });
    let focus = focus.flatten();
    let arrows = [
        (egui::Key::ArrowLeft, (-1, 0)),
        (egui::Key::ArrowRight, (1, 0)),
        (egui::Key::ArrowUp, (0, -1)),
        (egui::Key::ArrowDown, (0, 1)),
    ];
    for (room, cells) in rooms.iter().zip(&shown) {
        egui::Area::new(area_id(room.slot))
            .order(egui::Order::Background)
            .fixed_pos(room.rect.min)
            .constrain(false)
            .fade_in(false)
            .show(ctx, |ui| {
                ui.set_clip_rect(room.rect);
                // the whole room is the desktop's: a margin has no world to click
                let back = ui.interact(room.rect, area_id(room.slot).with("room"), Sense::click());
                ui.expand_to_include_rect(room.rect);
                if back.clicked() {
                    state.selected = None;
                }
                if back.secondary_clicked() {
                    state.menu_cell = pointer.and_then(|pos| room.grid.cell_at(pos)).map(|cell| (room.slot, cell));
                }
                room_menu(&back, env.p, desktop, room, cells, state.menu_cell, &mut ops);
                for &(id, cell) in cells {
                    let Some(shortcut) = desktop.shortcut(id) else { continue };
                    let face = targets::face(shortcut, &env.facts, &env.folders);
                    let is_folder = matches!(shortcut.target, Target::Folder { .. });
                    let own_icon = shortcut.icon.clone();
                    let rect = room.grid.rect(cell);
                    let open = || match face.folder {
                        Some(folder) => Op::OpenFolder {
                            folder,
                            cell: rect,
                            room: room.rect,
                        },
                        None => Op::Open(id),
                    };
                    let response = ui.interact(rect, shortcut_id(room.slot, id), Sense::click_and_drag());
                    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, face.greyed.is_none(), &face.name));
                    if focus == Some(id) {
                        response.request_focus();
                        state.selected = Some(id);
                    }
                    // (RLVa hides a folder's name: none is typed over it then)
                    let editable = face.greyed != Some(Greyed::Rlva);
                    let mut rename = false;
                    if response.has_focus() {
                        // the arrows go from shortcut to shortcut, not
                        // through egui's own order
                        ui.memory_mut(|m| {
                            m.set_focus_lock_filter(
                                response.id,
                                egui::EventFilter {
                                    horizontal_arrows: true,
                                    vertical_arrows: true,
                                    ..Default::default()
                                },
                            );
                        });
                        let pressed = ui.input(|i| arrows.iter().find(|(key, _)| i.key_pressed(*key)).map(|(_, step)| *step));
                        if let Some(next) = pressed.and_then(|step| super::place::neighbour(cells, cell, step)) {
                            ui.memory_mut(|m| m.request_focus(shortcut_id(room.slot, next)));
                            state.selected = Some(next);
                        }
                        rename = editable && ui.input(|i| i.key_pressed(egui::Key::F2));
                    }
                    let by_pointer = response.clicked_by(egui::PointerButton::Primary);
                    // (Enter or Space on the focused one, or the screen reader's click)
                    let by_key = response.clicked() && !by_pointer;
                    if by_pointer {
                        state.selected = Some(id);
                    }
                    let opened = by_key
                        || if desktop.single_click {
                            by_pointer
                        } else {
                            response.double_clicked()
                        };
                    if opened && face.greyed.is_none() {
                        ops.push(open());
                    }
                    if response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Delete)) {
                        ops.push(Op::Remove(id));
                    }
                    if response.drag_started_by(egui::PointerButton::Primary) {
                        state.drag = Some(id);
                        state.selected = Some(id);
                    }
                    let dragging = state.drag == Some(id) && response.dragged();
                    if dragging {
                        dragged = Some((face.clone(), room.grid.cell));
                    }
                    if state.drag == Some(id) && response.drag_stopped() {
                        state.drag = None;
                        if let Some((slot, cell)) = pointer.and_then(|pos| cell_under(rooms, pos)) {
                            ops.push(Op::Drop { id, slot, cell });
                        }
                    }
                    let look = Look {
                        hovered: response.hovered() && state.drag.is_none(),
                        selected: state.selected == Some(id),
                        focused: response.has_focus(),
                        dim: dragging,
                    };
                    paint_face(ui.painter(), ui.style(), env.p, env.icons, rect, &face, desktop.size, look);
                    let of = MenuOf {
                        id,
                        face: &face,
                        is_folder,
                        editable,
                        own_icon: own_icon.as_deref(),
                        slot: room.slot,
                        rooms,
                    };
                    rename |= shortcut_menu(&response, env.p, &of, open, &mut ops);
                    if rename {
                        state.rename = Some(id);
                        state.rename_fresh = true;
                        state.selected = Some(id);
                        ctx.data_mut(|d| d.insert_temp(rename_id(), face.name.clone()));
                    } else if state.rename == Some(id) {
                        // the name edited in place: Enter (or a click
                        // elsewhere) keeps it, Esc leaves it as it was
                        let mut text = rename_text(ctx);
                        let top = rect.top() + 8.0 + desktop.size.icon() + 2.0;
                        let at = Rect::from_min_max(egui::pos2(rect.left() + 2.0, top), egui::pos2(rect.right() - 2.0, top + 22.0));
                        let field = ui.put(
                            at,
                            egui::TextEdit::singleline(&mut text)
                                .id(area_id(room.slot).with(("rename", id)))
                                .char_limit(MAX_NAME)
                                .horizontal_align(egui::Align::Center)
                                .font(egui::TextStyle::Small),
                        );
                        if state.rename_fresh {
                            state.rename_fresh = false;
                            field.request_focus();
                            if let Some(mut edit) = egui::TextEdit::load_state(ctx, field.id) {
                                edit.cursor.set_char_range(Some(egui::text::CCursorRange::two(
                                    egui::text::CCursor::new(0),
                                    egui::text::CCursor::new(text.chars().count()),
                                )));
                                edit.store(ctx, field.id);
                            }
                            ctx.data_mut(|d| d.insert_temp(rename_id(), text));
                        } else if field.lost_focus() || escape {
                            state.rename = None;
                            if !escape {
                                ops.push(Op::Rename {
                                    id,
                                    text,
                                    own: face.target_name.clone(),
                                });
                            }
                            ctx.data_mut(|d| d.remove_temp::<String>(rename_id()));
                            // ended by a key: the keyboard stays on the
                            // shortcut (a click elsewhere took it already)
                            if escape || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                response.request_focus();
                            }
                        } else {
                            ctx.data_mut(|d| d.insert_temp(rename_id(), text));
                        }
                        continue;
                    }
                    if state.drag.is_none() && cargo.is_none() && button.is_none() {
                        // (built only while it shows; a folder's path too)
                        response.on_hover_ui(|ui| {
                            ui.label(match face.folder {
                                Some(folder) => face.tip_of(&crate::ui::inventory::inventory_path(env.folders.inv, folder)),
                                None => face.tip(),
                            });
                        });
                    }
                }
                // Inventory held over this desktop: what a drop would do
                let over = pointer.filter(|pos| room.rect.contains(*pos) && ctx.layer_id_at(*pos) == Some(ui.layer_id()));
                let mut dropping = false;
                if let (Some(ids), Some(pos)) = (&cargo, over) {
                    let offer = targets::offer(env.folders.inv, ids, env.folders.no_inventory);
                    dropping = offer.is_ok();
                    crate::ui::inventory::drag::feedback(
                        ctx,
                        if dropping {
                            egui::CursorIcon::Copy
                        } else {
                            egui::CursorIcon::NotAllowed
                        },
                    );
                    ui.painter().rect_stroke(
                        room.rect.shrink(1.0),
                        ui.visuals().widgets.inactive.corner_radius,
                        Stroke::new(1.5, if dropping { env.p.violet_light } else { env.p.danger }),
                        StrokeKind::Inside,
                    );
                    if ui.input(|i| i.pointer.any_released()) {
                        // taken here: nothing of it reaches the world
                        egui::DragAndDrop::clear_payload(ctx);
                        ops.push(Op::DropFolders {
                            slot: room.slot,
                            cell: room.grid.cell_at(pos),
                            offer,
                        });
                    }
                }
                // a toolbar button held over this desktop: a copy of it
                // (the button stays on its toolbar)
                if let (Some(command), Some(pos)) = (&button, over) {
                    let refused = targets::refuses_adding(command);
                    dropping = refused.is_none();
                    ctx.set_cursor_icon(if dropping {
                        egui::CursorIcon::Copy
                    } else {
                        egui::CursorIcon::NotAllowed
                    });
                    ui.painter().rect_stroke(
                        room.rect.shrink(1.0),
                        ui.visuals().widgets.inactive.corner_radius,
                        Stroke::new(1.5, if dropping { env.p.violet_light } else { env.p.danger }),
                        StrokeKind::Inside,
                    );
                    if ui.input(|i| i.pointer.any_released()) {
                        // taken here: no toolbar gets it
                        egui::DragAndDrop::clear_payload(ctx);
                        ops.push(match refused {
                            Some(why) => Op::Say(why.text().to_owned()),
                            None => Op::Add {
                                command: command.clone(),
                                slot: room.slot,
                                cell: room.grid.cell_at(pos),
                            },
                        });
                    }
                }
                // where the dragged shortcut, or what is dropped, would land
                if (state.drag.is_some() || dropping)
                    && let Some(cell) = pointer.and_then(|pos| room.grid.cell_at(pos))
                {
                    ui.painter().rect_stroke(
                        room.grid.rect(cell).shrink(2.0),
                        ui.visuals().widgets.inactive.corner_radius,
                        Stroke::new(1.5, env.p.violet_light),
                        StrokeKind::Inside,
                    );
                }
            });
    }
    // a drag whose shortcut is no longer drawn (its room went) ends there
    if state.drag.is_some() && !ctx.input(|i| i.pointer.any_down()) {
        state.drag = None;
    }
    if let (Some((face, cell)), Some(pos)) = (dragged, pointer) {
        let size = desktop.size;
        crate::ui::widgets::drag_ghost(ctx, env.p, "folderstorm_desktop_ghost", pos - cell / 2.0, |ui| {
            let (rect, _) = ui.allocate_exact_size(cell, Sense::hover());
            paint_face(ui.painter(), ui.style(), env.p, env.icons, rect, &face, size, Look::default());
        });
    }
    let mut out = Vec::new();
    let mut changed = false;
    for op in ops {
        match op {
            Op::Open(id) => match desktop.shortcut(id).map(|s| &s.target) {
                Some(Target::Command { command }) => out.push(Request::Run(command.clone())),
                // (a folder that can open asks with `Op::OpenFolder`)
                Some(Target::Folder { .. }) | None => {}
            },
            Op::OpenFolder { folder, cell, room } => out.push(Request::OpenFolder { folder, cell, room }),
            Op::ShowFolder(folder) => out.push(Request::ShowFolder(folder)),
            Op::DropFolders { slot, cell, offer } => match offer {
                Ok(picked) => {
                    let added = add_folders(desktop, rooms, &picked.folders, Some(slot), cell);
                    changed |= added.added > 0;
                    if added.first.is_some() {
                        state.selected = added.first;
                    }
                    if let Some(text) = super::added_notice(&added, picked.left_out, false, true) {
                        out.push(Request::Notice(text));
                    }
                }
                Err(why) => out.push(Request::Notice(why.text().to_owned())),
            },
            Op::Remove(id) => {
                changed |= desktop.remove(id).is_ok();
                if state.selected == Some(id) {
                    state.selected = None;
                }
            }
            Op::Add { command, slot, cell } => {
                let Some(at) = rooms.iter().position(|room| room.slot == slot) else {
                    continue;
                };
                let taken: Vec<_> = shown[at].iter().map(|(_, cell)| *cell).collect();
                match add_command(desktop, &rooms[at], &taken, &command, cell) {
                    CommandAdded::New(id) => {
                        state.selected = Some(id);
                        changed = true;
                    }
                    CommandAdded::Existing(id) => state.selected = Some(id),
                    CommandAdded::Refused(why) => out.push(Request::Notice(why.text())),
                    CommandAdded::Never(why) => out.push(Request::Notice(why.text().to_owned())),
                }
            }
            Op::Rename { id, text, own } => {
                // its target's own name typed back: no name of its own
                let text = super::store::clean_name(&text);
                changed |= desktop.rename(id, if text == own { "" } else { &text });
            }
            Op::Icon(id, icon) => changed |= desktop.set_icon(id, icon),
            Op::MoveTo { id, slot } => {
                let Some(room) = rooms.iter().find(|room| room.slot == slot) else {
                    continue;
                };
                let Some(target) = desktop.shortcut(id).map(|s| s.target.clone()) else {
                    continue;
                };
                if let Some(there) = existing(desktop, slot, &target) {
                    state.selected = Some(there);
                    out.push(Request::Notice(tr!("fs-desktop-add-existing").to_owned()));
                } else if let Some(cell) = room.grid.first_free(&taken_in(desktop, room)) {
                    changed |= desktop.place(id, slot, cell);
                } else {
                    out.push(Request::Notice(NoRoom::Full.text()));
                }
            }
            Op::Arrange(slot) => {
                if let Some(room) = rooms.iter().find(|room| room.slot == slot) {
                    let order = by_name(desktop, env, slot);
                    changed |= desktop.arrange(slot, &order, room.grid.rows);
                }
            }
            Op::ToolbarButtons => out.push(Request::ToolbarButtons),
            Op::Say(text) => out.push(Request::Notice(text)),
            Op::Drop { id, slot, cell } => {
                let from = rooms
                    .iter()
                    .zip(&shown)
                    .find_map(|(room, cells)| cells.iter().find(|(other, _)| *other == id).map(|(_, cell)| (room.slot, *cell)));
                let occupant = rooms
                    .iter()
                    .zip(&shown)
                    .filter(|(room, _)| room.slot == slot)
                    .find_map(|(_, cells)| cells.iter().find(|(_, at)| *at == cell).map(|(other, _)| *other))
                    .filter(|other| *other != id);
                if desktop.place(id, slot, cell) {
                    changed = true;
                    // the two swap
                    if let (Some(other), Some((slot, cell))) = (occupant, from) {
                        desktop.place(other, slot, cell);
                    }
                }
            }
            Op::Size(size) => {
                changed |= desktop.size != size;
                desktop.size = size;
            }
            Op::Settings => out.push(Request::Settings),
        }
    }
    if changed {
        out.push(Request::Changed);
    }
    if state != before {
        ctx.data_mut(|d| d.insert_temp(state_id(), state));
    }
    out
}

/// The room and the cell under `pos`.
fn cell_under(rooms: &[Room], pos: Pos2) -> Option<(u8, (u16, u16))> {
    rooms.iter().find_map(|room| room.grid.cell_at(pos).map(|cell| (room.slot, cell)))
}

/// The shortcut a menu is for.
struct MenuOf<'a> {
    id: u32,
    face: &'a Face,
    is_folder: bool,
    /// Its name and icon can be changed now (RLVa hides a folder's).
    editable: bool,
    /// The icon of its own, when it has one.
    own_icon: Option<&'a str>,
    slot: u8,
    rooms: &'a [Room],
}

/// The name of the monitor of `slot`, as the menus say it.
pub fn monitor_label(slot: u8) -> String {
    tr!("fs-desktop-monitor", number = u32::from(slot) + 1)
}

/// The menu of a shortcut; `open` is what opening it asks. True when
/// *Rename* was chosen.
fn shortcut_menu(response: &egui::Response, p: &Palette, of: &MenuOf, open: impl Fn() -> Op, ops: &mut Vec<Op>) -> bool {
    let (id, face) = (of.id, of.face);
    let mut rename = false;
    menu::context_menu(response, p, |ui| {
        let why = face.greyed.map_or("", |why| why.text());
        if menu::item_tip(ui, p, ICON_OPEN, tr!("fs-desktop-open"), face.greyed.is_none(), why) {
            ops.push(open());
        }
        if of.is_folder && menu::item_tip(ui, p, ICON_SHOW, tr!("ui-context-show-in-inventory"), face.folder.is_some(), why) {
            ops.extend(face.folder.map(Op::ShowFolder));
        }
        menu::separator(ui, p);
        let locked = if of.editable { "" } else { why };
        rename = menu::item_tip(ui, p, ICON_RENAME, tr!("fs-desktop-rename"), of.editable, locked);
        let icons = menu::submenu_heading(ui, p, ICON_ICON, tr!("fs-desktop-icon"), of.editable, |ui| {
            if menu::check(ui, p, ICON_DEFAULT, tr!("fs-desktop-icon-default"), of.own_icon.is_none(), true) {
                ops.push(Op::Icon(id, None));
            }
            menu::separator(ui, p);
            for line in super::icons::CHOICES.chunks(super::icons::PER_ROW) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for icon in line {
                        let cell = menu::icon_cell(ui, p, icon, "", true, false, vec2(28.0, 28.0));
                        cell.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, *icon));
                        if of.own_icon == Some(*icon) {
                            ui.painter().rect_stroke(
                                cell.rect.shrink(1.0),
                                ui.visuals().widgets.inactive.corner_radius,
                                Stroke::new(1.5, p.violet_light),
                                StrokeKind::Inside,
                            );
                        }
                        if cell.clicked() {
                            ops.push(Op::Icon(id, Some(*icon)));
                            ui.close();
                        }
                    }
                });
            }
        });
        if !of.editable {
            icons.on_hover_text(why);
        }
        // the drag is never the only way to another monitor
        let others: Vec<u8> = of.rooms.iter().map(|room| room.slot).filter(|slot| *slot != of.slot).collect();
        if !others.is_empty() {
            menu::submenu(ui, p, ICON_MOVE, tr!("fs-desktop-move-to"), true, |ui| {
                for slot in others {
                    if menu::item(ui, p, "", &monitor_label(slot)) {
                        ops.push(Op::MoveTo { id, slot });
                    }
                }
            });
        }
        menu::separator(ui, p);
        if menu::item(ui, p, ICON_REMOVE, tr!("fs-desktop-remove")) {
            ops.push(Op::Remove(id));
        }
    });
    rename
}

/// The menu of the desktop itself, opened on a cell without a shortcut
/// of `room`, whose shortcuts are shown in `cells`. *Add a command ›* is
/// greyed, with the reason, while that desktop takes no new shortcut.
fn room_menu(
    back: &egui::Response,
    p: &Palette,
    desktop: &Desktop,
    room: &Room,
    cells: &[(u32, (u16, u16))],
    menu_cell: Option<(u8, (u16, u16))>,
    ops: &mut Vec<Op>,
) {
    let (size, slot) = (desktop.size, room.slot);
    menu::context_menu(back, p, |ui| {
        // (worked out while the menu is open only)
        let taken: Vec<_> = cells.iter().map(|(_, cell)| *cell).collect();
        let refused = no_room(desktop, &room.grid, &taken);
        let add = menu::submenu_heading(ui, p, ICON_ADD, tr!("fs-desktop-add-command"), refused.is_none(), |ui| {
            let height = (ui.ctx().content_rect().height() * 0.6).max(160.0);
            egui::ScrollArea::vertical().max_height(height).show(ui, |ui| {
                for (command, label, icon, refused) in targets::addable() {
                    let why = refused.map_or("", |why| why.text());
                    if menu::item_tip(ui, p, icon, label, refused.is_none(), why) {
                        let cell = menu_cell.filter(|(at, _)| *at == slot).map(|(_, cell)| cell);
                        ops.push(Op::Add {
                            command: command.to_owned(),
                            slot,
                            cell,
                        });
                    }
                }
            });
        });
        if let Some(why) = refused {
            add.on_hover_text(why.text());
        }
        // Aurora's window of every command, to drag one from
        if menu::item(ui, p, ICON_TOOLBAR, tr!("toolbar-edit")) {
            ops.push(Op::ToolbarButtons);
        }
        menu::separator(ui, p);
        if menu::item_if(ui, p, ICON_ARRANGE, tr!("fs-desktop-arrange"), cells.len() > 1) {
            ops.push(Op::Arrange(slot));
        }
        menu::submenu(ui, p, ICON_SIZE, tr!("fs-desktop-size"), true, |ui| {
            for choice in Size::ALL {
                if menu::check(ui, p, "", size_label(choice), choice == size, true) {
                    ops.push(Op::Size(choice));
                }
            }
        });
        menu::separator(ui, p);
        if menu::item(ui, p, ICON_SETTINGS, tr!("fs-desktop-settings")) {
            ops.push(Op::Settings);
        }
    });
}

/// How a shortcut is drawn now.
#[derive(Clone, Copy, Debug, Default)]
struct Look {
    hovered: bool,
    selected: bool,
    focused: bool,
    /// Being dragged: its ghost stands for it.
    dim: bool,
}

/// A shortcut's face in `rect` (a cell): its icon over its name, on the
/// theme's pill when it is hovered or selected. The name stands on a
/// discreet pill of its own, so that it reads over any wallpaper.
#[allow(clippy::too_many_arguments)]
fn paint_face(painter: &egui::Painter, style: &egui::Style, p: &Palette, icons: &Icons, rect: Rect, face: &Face, size: Size, look: Look) {
    let mut painter = painter.clone();
    if look.dim {
        painter.multiply_opacity(0.35);
    }
    let radius = style.visuals.widgets.inactive.corner_radius;
    let pill = rect.shrink(2.0);
    if look.selected {
        painter.rect_filled(pill, radius, p.raised);
    } else if look.hovered {
        painter.rect_filled(pill, radius, p.raised.gamma_multiply(0.6));
    }
    if look.focused {
        painter.rect_stroke(pill, radius, Stroke::new(1.0, p.violet_light), StrokeKind::Inside);
    }
    let greyed = face.greyed.is_some();
    let side = size.icon();
    let icon = Rect::from_min_size(egui::pos2(rect.center().x - side / 2.0, rect.top() + 8.0), vec2(side, side));
    // a folder wears the colour the user gave it in the Inventory
    let own = face.tint.map(|[r, g, b]| egui::Color32::from_rgb(r, g, b));
    let tint = if greyed { p.muted_dim } else { own.unwrap_or(p.violet_light) };
    match icons.get(&face.icon).cloned().or_else(|| crate::ui::icons::global(&face.icon)) {
        Some(texture) => {
            let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            painter.image(texture.id(), icon, uv, tint);
        }
        // no such icon in this skin: its place is kept
        None => {
            painter.rect_stroke(icon.shrink(side / 4.0), radius, Stroke::new(1.0, tint), StrokeKind::Inside);
        }
    }
    // the name: two lines at most, cut with "…"
    let font = if size == Size::Small {
        egui::TextStyle::Small.resolve(style)
    } else {
        egui::TextStyle::Body.resolve(style)
    };
    let ink = if greyed { p.muted } else { p.ink };
    let mut job = egui::text::LayoutJob::simple(face.name.clone(), font, ink, rect.width() - 10.0);
    job.halign = egui::Align::Center;
    job.wrap.max_rows = 2;
    job.wrap.break_anywhere = false;
    job.wrap.overflow_character = Some('…');
    let galley = painter.layout_job(job);
    let at = egui::pos2(rect.center().x, icon.bottom() + 4.0);
    let text = galley.rect.translate(at.to_vec2()).expand2(vec2(3.0, 1.0)).intersect(rect);
    painter.rect_filled(text, radius, p.panel.gamma_multiply(0.85));
    painter.galley(at, galley, ink);
}

/// The cell [`show`] draws the shortcut `id` in, for the tests.
#[cfg(test)]
pub(super) fn shown_cell(desktop: &Desktop, room: &Room, id: u32) -> Option<(u16, u16)> {
    let stored: Vec<_> = desktop
        .shortcuts
        .iter()
        .filter(|s| s.slot == room.slot)
        .map(|s| (s.id, s.cell()))
        .collect();
    super::place::shown(&stored, &room.grid)
        .into_iter()
        .find(|(other, _)| *other == id)
        .map(|(_, cell)| cell)
}
