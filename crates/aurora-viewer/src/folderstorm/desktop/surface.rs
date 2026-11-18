//! The desktops as drawn: one area per room, on the layer between the
//! margins' background and the windows, its shortcuts on the grid, their
//! hover, selection, drag between cells and menus, and the Inventory
//! folders dropped on it
//! (folderstorm/plans/margin-desktop.md, "Drawing order and input").

use super::place::Grid;
use super::store::{Desktop, Error, Size, Target};
use super::targets::{self, Face};
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

fn drawn_id() -> egui::Id {
    egui::Id::unique("folderstorm_desktop_drawn_shortcuts")
}

/// The rectangles of the shortcuts drawn by the last [`show`] (none while
/// no desktop shows: `super::show` clears them).
pub(super) fn drawn_shortcuts(ctx: &egui::Context) -> Vec<Rect> {
    ctx.data(|d| d.get_temp::<Vec<Rect>>(drawn_id())).unwrap_or_default()
}

/// Sets what [`drawn_shortcuts`] gives.
pub(super) fn set_drawn_shortcuts(ctx: &egui::Context, rects: Vec<Rect>) {
    ctx.data_mut(|d| d.insert_temp(drawn_id(), rects));
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
}

fn state_id() -> egui::Id {
    egui::Id::unique("folderstorm_desktop_state")
}

pub(super) fn state(ctx: &egui::Context) -> State {
    ctx.data(|d| d.get_temp(state_id())).unwrap_or_default()
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
        command: &'static str,
        slot: u8,
        cell: Option<(u16, u16)>,
    },
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

/// The icons this module names, for the test that they are embedded.
#[cfg(test)]
pub(super) const ICONS: &[&str] = &[ICON_OPEN, ICON_REMOVE, ICON_ADD, ICON_SIZE, ICON_SETTINGS, ICON_SHOW];

/// What adding folders to a desktop did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Added {
    /// New shortcuts.
    pub added: usize,
    /// Folders that desktop has a shortcut to already.
    pub existing: usize,
    /// Folders left out: no free cell, or the profile's limit.
    pub no_room: usize,
    /// The first shortcut added, else the first one found there already.
    pub first: Option<u32>,
}

/// Without a room (the world fills the window), shortcuts are kept in
/// columns of this many cells; a room shows them where they fit.
const UNSEEN_ROWS: u16 = 8;

/// Adds a shortcut to each of `folders` (of `account`) in the desktop of
/// `slot` (None: the first room's): from `cell` when one is given, each in
/// the free cell nearest to it; else in the first free cells. Without a
/// room for that slot they are kept for when it has one.
pub fn add_folders(
    desktop: &mut Desktop,
    rooms: &[Room],
    account: Uuid,
    folders: &[Uuid],
    slot: Option<u8>,
    cell: Option<(u16, u16)>,
) -> Added {
    let slot = slot.or(rooms.first().map(|room| room.slot)).unwrap_or(0);
    let grid = rooms.iter().find(|room| room.slot == slot).map(|room| room.grid);
    let mut out = Added::default();
    for (n, folder) in folders.iter().enumerate() {
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
            out.no_room += folders.len() - n;
            break;
        };
        let target = Target::Folder { folder: *folder, account };
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
                out.no_room += folders.len() - n;
                break;
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
    let mut ops = Vec::new();
    let mut dragged = None;
    let mut drawn = Vec::new();
    for (room, cells) in rooms.iter().zip(&shown) {
        egui::Area::new(area_id(room.slot))
            .order(egui::Order::Background)
            .fixed_pos(room.rect.min)
            .constrain(false)
            .fade_in(false)
            .show(ctx, |ui| {
                ui.set_clip_rect(room.rect);
                // the whole room is the desktop's: a margin has no world to
                // click, only a HUD moved there (`env.hud_at_pointer`)
                let sense = if env.hud_at_pointer { Sense::hover() } else { Sense::click() };
                let back = ui.interact(room.rect, area_id(room.slot).with("room"), sense);
                ui.expand_to_include_rect(room.rect);
                if back.clicked() {
                    state.selected = None;
                }
                if back.secondary_clicked() {
                    state.menu_cell = pointer.and_then(|pos| room.grid.cell_at(pos)).map(|cell| (room.slot, cell));
                }
                room_menu(&back, env.p, desktop.size, room.slot, state.menu_cell, &mut ops);
                for &(id, cell) in cells {
                    let Some(shortcut) = desktop.shortcut(id) else { continue };
                    let face = targets::face(shortcut, &env.facts, &env.folders);
                    let is_folder = matches!(shortcut.target, Target::Folder { .. });
                    let rect = room.grid.rect(cell);
                    drawn.push(rect.intersect(room.rect));
                    let open = || match face.folder {
                        Some(folder) => Op::OpenFolder {
                            folder,
                            cell: rect,
                            room: room.rect,
                        },
                        None => Op::Open(id),
                    };
                    let response = ui.interact(rect, area_id(room.slot).with(("shortcut", id)), Sense::click_and_drag());
                    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, face.greyed.is_none(), &face.name));
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
                    shortcut_menu(&response, env.p, id, &face, is_folder, open, &mut ops);
                    if state.drag.is_none() && cargo.is_none() {
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
                // where the dragged shortcut, or the folder dropped, would land
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
    set_drawn_shortcuts(ctx, drawn);
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
                    let added = add_folders(desktop, rooms, env.folders.account, &picked.folders, Some(slot), cell);
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
                let cell = cell
                    .filter(|cell| !taken.contains(cell))
                    .or_else(|| rooms[at].grid.first_free(&taken));
                // (a room with no free cell takes no more)
                let Some(cell) = cell else { continue };
                match desktop.add(Target::command(command), slot, cell) {
                    Ok(id) => {
                        state.selected = Some(id);
                        changed = true;
                    }
                    // the one that desktop has already is shown instead
                    Err(Error::Existing(id)) => state.selected = Some(id),
                    Err(Error::Full | Error::Missing) => {}
                }
            }
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

/// The menu of a shortcut; `open` is what opening it asks.
fn shortcut_menu(response: &egui::Response, p: &Palette, id: u32, face: &Face, is_folder: bool, open: impl Fn() -> Op, ops: &mut Vec<Op>) {
    menu::context_menu(response, p, |ui| {
        let why = face.greyed.map_or("", |why| why.text());
        if menu::item_tip(ui, p, ICON_OPEN, tr!("fs-desktop-open"), face.greyed.is_none(), why) {
            ops.push(open());
        }
        if is_folder && menu::item_tip(ui, p, ICON_SHOW, tr!("ui-context-show-in-inventory"), face.folder.is_some(), why) {
            ops.extend(face.folder.map(Op::ShowFolder));
        }
        menu::separator(ui, p);
        if menu::item(ui, p, ICON_REMOVE, tr!("fs-desktop-remove")) {
            ops.push(Op::Remove(id));
        }
    });
}

/// The menu of the desktop itself, opened on a cell without a shortcut.
fn room_menu(back: &egui::Response, p: &Palette, size: Size, slot: u8, menu_cell: Option<(u8, (u16, u16))>, ops: &mut Vec<Op>) {
    menu::context_menu(back, p, |ui| {
        menu::submenu(ui, p, ICON_ADD, tr!("fs-desktop-add-command"), true, |ui| {
            let height = (ui.ctx().content_rect().height() * 0.6).max(160.0);
            egui::ScrollArea::vertical().max_height(height).show(ui, |ui| {
                for (command, label, icon, refused) in targets::addable() {
                    let why = refused.map_or("", |why| why.text());
                    if menu::item_tip(ui, p, icon, label, refused.is_none(), why) {
                        let cell = menu_cell.filter(|(at, _)| *at == slot).map(|(_, cell)| cell);
                        ops.push(Op::Add { command, slot, cell });
                    }
                }
            });
        });
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
