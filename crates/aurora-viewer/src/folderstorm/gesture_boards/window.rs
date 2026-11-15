//! The gesture board window: one strip on top (board, search, add,
//! options), the page of tiles, and previous / next when the board has
//! more than one page (Folderstorm (C++)'s "compact board refinement").
//! What is clicked to play is left to the app as a [`Request`]; changes to
//! the boards are made in the account given, which the app saves.

use super::dialogs::{self, Dialog};
use super::layout::{self, Hit, Page};
use super::store::{Account, Board};
use super::{Item, Kind, WINDOW, resolve};
use crate::theme::Palette;
use crate::ui::inventory::InvDrag;
use crate::ui::widgets::{self, Floater};
use crate::world::inventory::Inventory;
use aurora_i18n::tr;
use egui::{Color32, Rect, Sense, Stroke, Vec2, pos2, vec2};
use std::collections::BTreeMap;
use uuid::Uuid;

/// What the window knows of the viewer.
pub struct Facts<'a> {
    pub inv: &'a Inventory,
    /// Whether a gesture (an original item id) is playing now.
    pub playing: &'a dyn Fn(Uuid) -> bool,
    /// Why gestures cannot be played now (RLVa `@sendgesture`, gestures
    /// turned off); None when they can.
    pub gestures_blocked: Option<&'a str>,
}

/// What the window asks of the app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// Play (or, for a gesture playing already, stop) an item: the original
    /// of a link.
    Play { original: Uuid, kind: Kind },
    /// Show the item in the Inventory window.
    ShowInventory(Uuid),
}

/// Height of the top strip and of the pages' row.
const STRIP: f32 = 22.0;
/// How long a tile found or added again stays outlined, in seconds.
const FLASH: f64 = 1.6;
/// How long a message stays, in seconds.
const MESSAGE: f64 = 5.0;

/// The window's state between frames (egui's memory).
#[derive(Clone, Default)]
pub(super) struct View {
    /// The page shown, by board.
    pages: BTreeMap<u32, usize>,
    /// The search, while it is open.
    pub(super) search: Option<Search>,
    /// A tile whose page is to be shown.
    pub(super) reveal: Option<u32>,
    /// A tile outlined for a moment (found, or added again), and when.
    flash: Option<(u32, f64)>,
    /// A message, and when it was given.
    message: Option<(String, f64)>,
    /// The dialog open over the window.
    pub(super) dialog: Option<Dialog>,
}

#[derive(Clone, Default)]
pub(super) struct Search {
    pub(super) text: String,
    focus: bool,
    page: usize,
}

impl View {
    pub(super) fn say(&mut self, ctx: &egui::Context, text: String) {
        self.message = Some((text, ctx.input(|i| i.time)));
    }

    /// Outlines `tile` for a moment and shows its page.
    pub(super) fn flash(&mut self, ctx: &egui::Context, tile: u32) {
        self.flash = Some((tile, ctx.input(|i| i.time)));
        self.reveal = Some(tile);
    }
}

/// A tile being dragged to another place on its board.
#[derive(Clone, Copy)]
struct TileDrag {
    board: u32,
    tile: u32,
}

pub(super) fn view_id() -> egui::Id {
    egui::Id::unique("fs_gesture_board_view")
}

pub(super) fn search_field_id() -> egui::Id {
    egui::Id::unique("fs_gesture_board_search")
}

/// The demo's search: open, with `text` typed.
pub fn demo_search(ctx: &egui::Context, text: &str) {
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<View>(view_id()).search = Some(Search {
            text: text.to_owned(),
            ..Default::default()
        });
    });
}

/// Draws the window when it is open: returns what was clicked to play.
pub fn show(ctx: &egui::Context, p: &Palette, account: &mut Account, facts: &Facts) -> Vec<Request> {
    let mut out = Vec::new();
    if !super::is_open(ctx) {
        return out;
    }
    let mut view: View = ctx.data_mut(|d| std::mem::take(d.get_temp_mut_or_default::<View>(view_id())));
    let mut open = true;
    let area = crate::ui::layout::work_area(ctx);
    Floater::new(
        WINDOW,
        tr!("fs-gesture-board-title"),
        pos2(area.left() + 90.0, area.top() + 140.0),
        vec2(330.0, 210.0),
    )
    .help(tr!("fs-gesture-board-help"))
    .steady_size()
    .show(ctx, p, &mut open, |ui| body(ui, p, account, facts, &mut view, &mut out));
    if !open {
        super::set_open(ctx, false);
        view.search = None;
        view.dialog = None;
    }
    dialogs::show(ctx, p, account, facts, &mut view);
    ctx.data_mut(|d| d.insert_temp(view_id(), view));
    out
}

fn body(ui: &mut egui::Ui, p: &Palette, account: &mut Account, facts: &Facts, view: &mut View, out: &mut Vec<Request>) {
    let now = ui.input(|i| i.time);
    if view.message.as_ref().is_some_and(|(_, at)| now - at > MESSAGE) {
        view.message = None;
    }
    if view.flash.is_some_and(|(_, at)| now - at > FLASH) {
        view.flash = None;
    }
    if view.message.is_some() || view.flash.is_some() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
    }
    // Ctrl+F for this window (`ui::find_shortcut`): the search, its field
    // with the keyboard
    if crate::ui::find_shortcut::take(ui, &[search_field_id()]) {
        let search = view.search.get_or_insert_with(Search::default);
        search.focus = true;
    }
    strip(ui, p, account, view);
    if let Some(search) = &mut view.search {
        let mut close = false;
        ui.horizontal(|ui| {
            let room = ui.available_width();
            let only = tr!("fs-gesture-board-search-current");
            let only_width = ui
                .painter()
                .layout_no_wrap(only.to_owned(), egui::TextStyle::Body.resolve(ui.style()), p.ink)
                .size()
                .x
                + 28.0;
            let field_width = (room - only_width - ui.spacing().item_spacing.x).max(80.0);
            let response = widgets::search_text_edit(ui, p, &mut search.text, |edit| {
                edit.id(search_field_id())
                    .hint_text(tr!("fs-gesture-board-search-hint"))
                    .desired_width(field_width - 8.0)
            });
            if search.focus {
                response.request_focus();
                search.focus = false;
            }
            if response.changed() {
                search.page = 0;
            }
            if (response.has_focus() || response.lost_focus()) && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
            widgets::checkbox(ui, p, &mut account.search_current, only).on_hover_text(tr!("fs-gesture-board-search-current-help"));
        });
        if close {
            view.search = None;
        }
    }
    let footer = STRIP + ui.spacing().item_spacing.y;
    let top = ui.cursor().top();
    let full = Rect::from_min_max(pos2(ui.max_rect().left(), top), ui.max_rect().max);
    let room_with_footer = full.height() - footer;
    if view.search.is_some() {
        search_results(ui, p, account, facts, view, out, full, room_with_footer);
    } else {
        board_page(ui, p, account, facts, view, out, full, room_with_footer);
    }
}

/// The strip on top: the board shown, then search, add and options.
fn strip(ui: &mut egui::Ui, p: &Palette, account: &mut Account, view: &mut View) {
    ui.horizontal(|ui| {
        ui.set_height(STRIP);
        let buttons = 3.0 * (20.0 + ui.spacing().item_spacing.x) + 8.0;
        let combo_width = (ui.available_width() - buttons).max(60.0);
        let current = account.current_id();
        let name = account
            .current()
            .map_or_else(|| tr!("fs-gesture-board-none").to_owned(), |b| b.name.clone());
        let mut chosen = None;
        egui::ComboBox::from_id_salt("fs_gesture_board_pick")
            .width(combo_width)
            .selected_text(egui::RichText::new(name).color(p.ink))
            .truncate()
            .show_ui(ui, |ui| {
                for board in &account.boards {
                    if ui.selectable_label(Some(board.id) == current, &board.name).clicked() {
                        chosen = Some(board.id);
                    }
                }
                if account.boards.is_empty() {
                    ui.label(egui::RichText::new(tr!("fs-gesture-board-none")).color(p.muted));
                }
            });
        if let Some(id) = chosen {
            account.selected = Some(id);
        }
        let searching = view.search.is_some();
        if widgets::icon_toggle(ui, p, "search-filled", tr!("fs-gesture-board-search-tip"), searching) {
            view.search = if searching {
                None
            } else {
                Some(Search {
                    focus: true,
                    ..Default::default()
                })
            };
        }
        if widgets::icon_button(ui, p, "circle-plus-filled", tr!("fs-gesture-board-add-tip"), current.is_some()) {
            view.dialog = Some(Dialog::add());
        }
        options_menu(ui, p, account, view);
    });
}

fn options_menu(ui: &mut egui::Ui, p: &Palette, account: &mut Account, view: &mut View) {
    use crate::ui::menu;
    let current = account.current_id();
    widgets::icon_menu(ui, p, "settings-filled", tr!("fs-gesture-board-options"), |ui| {
        if menu::item(ui, p, "circle-plus-filled", tr!("fs-gesture-board-new")) {
            view.dialog = Some(Dialog::new_board());
        }
        if let Some(id) = current {
            let name = account.board(id).map(|b| b.name.clone()).unwrap_or_default();
            if menu::item(ui, p, "pencil-filled", tr!("fs-gesture-board-rename")) {
                view.dialog = Some(Dialog::rename(id, &name));
            }
            if menu::item(ui, p, "copy-filled", tr!("fs-gesture-board-duplicate")) {
                view.dialog = Some(Dialog::duplicate(id, &tr!("fs-gesture-board-copy-of", name = name.as_str())));
            }
            let at = account.boards.iter().position(|b| b.id == id).unwrap_or(0);
            if menu::item_if(ui, p, "arrow-left", tr!("fs-gesture-board-move-board-earlier"), at > 0) {
                account.move_board(id, -1);
            }
            if menu::item_if(
                ui,
                p,
                "arrow-right",
                tr!("fs-gesture-board-move-board-later"),
                at + 1 < account.boards.len(),
            ) {
                account.move_board(id, 1);
            }
            if menu::item(ui, p, "trash-filled", tr!("fs-gesture-board-delete")) {
                view.dialog = Some(Dialog::Delete(id));
            }
            menu::separator(ui, p);
            if menu::item(ui, p, "circle-plus-filled", tr!("fs-gesture-board-add")) {
                view.dialog = Some(Dialog::add());
            }
            if menu::item(ui, p, "search-filled", tr!("fs-gesture-board-find")) {
                view.search = Some(Search {
                    focus: true,
                    ..Default::default()
                });
            }
            if let Some(board) = account.board_mut(id) {
                menu::toggle(ui, p, "arrows-maximize", tr!("fs-gesture-board-full-names"), &mut board.full_names);
            }
            if menu::item(ui, p, "dimensions", tr!("fs-gesture-board-settings")) {
                view.dialog = Some(Dialog::Settings(id));
            }
        }
    });
}

/// How a tile is drawn.
struct Look<'a> {
    label: &'a str,
    kind: Option<Kind>,
    ready: bool,
    color: Option<[u8; 3]>,
    playing: bool,
    outlined: bool,
}

/// The label a tile shows: its own, else its item's name, else
/// "Unavailable".
pub(super) fn label_of(tile: &super::store::Tile, item: &Item) -> String {
    match (&tile.label, item) {
        (Some(label), _) => label.clone(),
        (None, Item::Ready { name, .. }) => name.clone(),
        (None, _) => tr!("fs-gesture-board-unavailable").to_owned(),
    }
}

/// The text colour that reads best on `fill`, among the palette's.
pub(super) fn ink_on(p: &Palette, fill: Color32) -> Color32 {
    fn luminance(c: Color32) -> f32 {
        let f = |v: u8| {
            let v = f32::from(v) / 255.0;
            if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
    }
    let contrast = |a: Color32| {
        let (x, y) = (luminance(a), luminance(fill));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    };
    [p.ink, p.on_accent, p.navy, p.panel]
        .into_iter()
        .map(|c| c.to_opaque())
        .max_by(|a, b| contrast(*a).total_cmp(&contrast(*b)))
        .unwrap_or(p.ink)
}

fn tile_font(ui: &egui::Ui) -> egui::FontId {
    egui::TextStyle::Button.resolve(ui.style())
}

/// The width of `label` in the tiles' font.
fn label_width(ui: &egui::Ui, label: &str) -> f32 {
    ui.painter()
        .layout_no_wrap(label.to_owned(), tile_font(ui), Color32::PLACEHOLDER)
        .size()
        .x
}

fn paint_tile(ui: &egui::Ui, p: &Palette, rect: Rect, response: &egui::Response, look: &Look) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter();
    let radius = ui.visuals().widgets.inactive.corner_radius;
    let hovered = response.hovered() && look.ready;
    let fill = match look.color {
        // theme-guard: content — the colour the user chose for this tile
        Some([r, g, b]) => Color32::from_rgb(r, g, b),
        None if look.playing => p.violet,
        None => ui.style().interact(response).weak_bg_fill,
    };
    let fill = match (look.color, hovered) {
        (Some(_), true) => fill.lerp_to_gamma(p.ink, 0.14),
        _ => fill,
    };
    let fill = if look.ready { fill } else { fill.gamma_multiply(0.45) };
    painter.rect_filled(rect, radius, fill);
    if (look.playing && look.color.is_some()) || look.outlined || response.has_focus() {
        painter.rect_stroke(rect, radius, Stroke::new(2.0, p.violet_light), egui::StrokeKind::Inside);
    }
    let ink = if !look.ready {
        p.muted_dim
    } else if look.playing && look.color.is_none() {
        p.on_accent
    } else {
        ink_on(p, fill)
    };
    let sound = look.kind == Some(Kind::Sound);
    let speaker = if sound { layout::SPEAKER } else { 0.0 };
    if sound {
        widgets::paint_glyph(
            painter,
            super::SOUND_ICON,
            pos2(rect.right() - layout::PAD - speaker / 2.0 + 2.0, rect.center().y),
            12.0,
            ink,
        );
    }
    let width = (rect.width() - 2.0 * layout::PAD - speaker).max(4.0);
    let mut job = egui::text::LayoutJob::single_section(look.label.to_owned(), egui::TextFormat::simple(tile_font(ui), ink));
    job.wrap = egui::text::TextWrapping::truncate_at_width(width);
    let galley = painter.layout_job(job);
    let left = rect.left() + layout::PAD + (width - galley.size().x).max(0.0) / 2.0;
    let at = pos2(left, rect.center().y - galley.size().y / 2.0);
    painter.with_clip_rect(rect).galley(at, galley, ink);
}

/// One tile's tooltip: its label, its item's name, what it is, why it
/// cannot play, how to use it.
fn tooltip(tile: &super::store::Tile, item: &Item, blocked: Option<&str>, board: Option<&str>) -> String {
    let mut lines = Vec::new();
    match item {
        Item::Ready { name, kind, .. } => {
            if tile.label.as_deref().is_some_and(|l| l != name) {
                lines.push(format!("{}\n{}", tile.label.as_deref().unwrap_or_default(), name));
            } else {
                lines.push(name.clone());
            }
            match kind {
                Kind::Gesture => {
                    lines.push(tr!("fs-gesture-board-kind-gesture").to_owned());
                    if let Some(why) = blocked {
                        lines.push(why.to_owned());
                    }
                }
                Kind::Sound => lines.push(tr!("fs-gesture-board-kind-sound").to_owned()),
            }
        }
        other => {
            lines.push(label_of(tile, other));
            lines.extend(other.why().map(str::to_owned));
        }
    }
    if let Some(board) = board {
        lines.push(tr!("fs-gesture-board-on-board", name = board));
    }
    lines.join("\n")
}

/// Sizes of the tiles of `board`, as wide as the room at most.
fn sizes(ui: &egui::Ui, board: &Board, items: &[Item], room: Vec2) -> Vec<Vec2> {
    board
        .tiles
        .iter()
        .zip(items)
        .map(|(tile, item)| {
            let size = tile.size_on(board);
            let sound = matches!(item, Item::Ready { kind: Kind::Sound, .. });
            let label = if board.full_names {
                label_width(ui, &label_of(tile, item))
            } else {
                0.0
            };
            vec2(layout::tile_width(board, size, label, sound, room.x), size[1].min(room.y))
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn board_page(
    ui: &mut egui::Ui,
    p: &Palette,
    account: &mut Account,
    facts: &Facts,
    view: &mut View,
    out: &mut Vec<Request>,
    full: Rect,
    without_footer: f32,
) {
    let Some(board_id) = account.current_id() else {
        empty(ui, p, full, tr!("fs-gesture-board-empty-boards"));
        if ui
            .interact(full, egui::Id::unique("fs_gesture_board_empty"), Sense::click())
            .clicked()
        {
            view.dialog = Some(Dialog::new_board());
        }
        return;
    };
    let Some(board) = account.board(board_id) else {
        return;
    };
    let items: Vec<Item> = board.tiles.iter().map(|t| resolve(facts.inv, t.item)).collect();
    // the pages as if there were no footer; with more than one, the
    // footer takes its row
    let mut room = full.size();
    let mut sizes_now = sizes(ui, board, &items, room);
    let mut pages = layout::pages(&sizes_now, room, usize::from(board.per_page));
    let footer = pages.len() > 1 || view.message.is_some();
    if footer {
        room.y = without_footer.max(1.0);
        sizes_now = sizes(ui, board, &items, room);
        pages = layout::pages(&sizes_now, room, usize::from(board.per_page));
    }
    let page_count = pages.len();
    let mut page = view.pages.get(&board_id).copied().unwrap_or(0);
    if let Some(index) = view.reveal.take().and_then(|tile| board.index_of(tile)) {
        page = layout::page_of(&pages, index);
    }
    page = page.min(page_count - 1);
    let room_rect = Rect::from_min_size(full.min, room);
    // an inventory gesture or sound dropped on the board: added where it
    // is dropped, or at the end
    let drop = inventory_drop(ui, p, room_rect);
    let mut drop_at: Option<Option<usize>> = drop.map(|_| None);
    let mut moved: Option<(usize, usize)> = None;
    let mut remove = None;
    let mut edit = None;
    let mut step: Option<(usize, isize)> = None;
    let dragging_tile = egui::DragAndDrop::payload::<TileDrag>(ui.ctx())
        .map(|d| *d)
        .filter(|d| d.board == board_id);
    if board.tiles.is_empty() {
        empty(ui, p, room_rect, tr!("fs-gesture-board-empty-tiles"));
    }
    for (index, rect) in pages.get(page).map(|pg| pg.tiles.as_slice()).unwrap_or_default() {
        let (tile, item) = (&board.tiles[*index], &items[*index]);
        let rect = rect.translate(room_rect.min.to_vec2());
        let response = ui.interact(rect, egui::Id::unique(("fs_gesture_board_tile", tile.id)), Sense::click_and_drag());
        let (ready_kind, original) = match item {
            Item::Ready { kind, original, .. } => (Some(*kind), Some(*original)),
            _ => (None, None),
        };
        let playing = ready_kind == Some(Kind::Gesture) && original.is_some_and(|o| (facts.playing)(o));
        let blocked = if ready_kind == Some(Kind::Gesture) {
            facts.gestures_blocked
        } else {
            None
        };
        let label = label_of(tile, item);
        paint_tile(
            ui,
            p,
            rect,
            &response,
            &Look {
                label: &label,
                kind: ready_kind,
                ready: original.is_some() && blocked.is_none(),
                color: tile.color,
                playing,
                outlined: view.flash.is_some_and(|(t, _)| t == tile.id),
            },
        );
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, original.is_some(), &label));
        if response.drag_started() {
            response.dnd_set_drag_payload(TileDrag {
                board: board_id,
                tile: tile.id,
            });
        }
        // a tile dragged over this one goes before it
        if let Some(drag) = dragging_tile
            && drag.tile != tile.id
            && ui.rect_contains_pointer(rect)
        {
            let x = rect.left() - layout::GAP / 2.0;
            ui.painter()
                .line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(2.0, p.violet_light));
            if ui.input(|i| i.pointer.any_released())
                && let Some(from) = board.index_of(drag.tile)
            {
                moved = Some((from, *index));
            }
        }
        if drop.is_some() && ui.rect_contains_pointer(rect) {
            drop_at = Some(Some(*index));
        }
        if response.clicked()
            && let (Some(original), Some(kind)) = (original, ready_kind)
            && blocked.is_none()
        {
            out.push(Request::Play { original, kind });
        }
        let tip = tooltip(tile, item, blocked, None);
        let response = response.on_hover_text(tip);
        response.context_menu(|ui| {
            use crate::ui::menu;
            if menu::item_if(
                ui,
                p,
                "player-play-filled",
                tr!("common-play"),
                original.is_some() && blocked.is_none(),
            ) && let (Some(original), Some(kind)) = (original, ready_kind)
            {
                out.push(Request::Play { original, kind });
            }
            if menu::item(ui, p, "pencil-filled", tr!("fs-gesture-board-edit")) {
                edit = Some(tile.id);
            }
            if menu::item_if(ui, p, "arrow-left", tr!("fs-gesture-board-move-earlier"), *index > 0) {
                step = Some((*index, -1));
            }
            if menu::item_if(
                ui,
                p,
                "arrow-right",
                tr!("fs-gesture-board-move-later"),
                *index + 1 < board.tiles.len(),
            ) {
                step = Some((*index, 1));
            }
            if menu::item_if(ui, p, "eye-filled", tr!("fs-gesture-board-show-inventory"), original.is_some())
                && let Some(original) = original
            {
                out.push(Request::ShowInventory(original));
            }
            if menu::item(ui, p, "trash-filled", tr!("fs-gesture-board-remove")) {
                remove = Some(tile.id);
            }
        });
    }
    // a tile dropped after the last one of the page, in the free room
    if let Some(drag) = dragging_tile
        && moved.is_none()
        && ui.rect_contains_pointer(room_rect)
        && ui.input(|i| i.pointer.any_released())
        && !pages[page]
            .tiles
            .iter()
            .any(|(_, r)| ui.rect_contains_pointer(r.translate(room_rect.min.to_vec2())))
        && let Some(from) = board.index_of(drag.tile)
    {
        moved = Some((from, pages[page].span().end));
    }
    if footer {
        let row = Rect::from_min_max(pos2(full.left(), room_rect.bottom() + ui.spacing().item_spacing.y), full.max);
        pages_row(ui, p, row, &mut page, page_count, view.message.as_ref().map(|(m, _)| m.as_str()));
    }
    view.pages.insert(board_id, page);
    // the changes, once the board is no longer borrowed
    if let Some(board) = account.board_mut(board_id) {
        if let Some((from, to)) = moved {
            board.move_tile(from, to);
            egui::DragAndDrop::clear_payload(ui.ctx());
        }
        if let Some((index, step)) = step {
            let to = if step < 0 { index - 1 } else { index + 2 };
            board.move_tile(index, to);
        }
    }
    if let Some(tile) = remove {
        let _ = account.remove_tile(board_id, tile);
    }
    if let Some(tile) = edit
        && let Some(dialog) = account.board(board_id).and_then(|b| Dialog::edit(b, tile))
    {
        view.dialog = Some(dialog);
    }
    if let (Some(id), Some(at)) = (drop, drop_at) {
        add_dropped(ui.ctx(), account, facts, view, board_id, id, at);
    }
}

/// An inventory item dropped on `room` this frame: the room is outlined
/// while one is carried over it.
fn inventory_drop(ui: &egui::Ui, p: &Palette, room: Rect) -> Option<Uuid> {
    let carried = egui::DragAndDrop::payload::<InvDrag>(ui.ctx())?;
    let over = ui.rect_contains_pointer(room)
        && ui
            .ctx()
            .pointer_latest_pos()
            .and_then(|at| ui.ctx().layer_id_at(at))
            .is_some_and(|layer| layer == ui.layer_id());
    if !over {
        return None;
    }
    ui.painter().rect_stroke(
        room.shrink(1.0),
        ui.visuals().widgets.inactive.corner_radius,
        Stroke::new(1.5, p.violet_light),
        egui::StrokeKind::Inside,
    );
    if ui.input(|i| i.pointer.any_released()) {
        egui::DragAndDrop::clear_payload(ui.ctx());
        return Some(carried.0);
    }
    None
}

/// Adds the dropped item `id` to the board, before the tile at `at`.
pub(super) fn add_dropped(
    ctx: &egui::Context,
    account: &mut Account,
    facts: &Facts,
    view: &mut View,
    board: u32,
    id: Uuid,
    at: Option<usize>,
) {
    match resolve(facts.inv, id) {
        Item::Ready { .. } => match account.add_items(board, &[id], at) {
            Ok(added) => {
                if let Some(tile) = added.added.first().or(added.existing.first()) {
                    view.flash(ctx, *tile);
                }
                if !added.existing.is_empty() {
                    view.say(ctx, tr!("fs-gesture-board-already").to_owned());
                }
            }
            Err(error) => view.say(ctx, error.text()),
        },
        _ => view.say(ctx, tr!("fs-gesture-board-drop-refused").to_owned()),
    }
}

fn empty(ui: &egui::Ui, p: &Palette, rect: Rect, text: &str) {
    let mut job = egui::text::LayoutJob::single_section(
        text.to_owned(),
        egui::TextFormat::simple(egui::TextStyle::Body.resolve(ui.style()), p.muted),
    );
    job.wrap.max_width = (rect.width() - 16.0).max(40.0);
    job.halign = egui::Align::Center;
    let galley = ui.painter().layout_job(job);
    let at = pos2(rect.center().x, rect.center().y - galley.size().y / 2.0);
    ui.painter().with_clip_rect(rect).galley(at, galley, p.muted);
}

/// Previous, "2 / 5", next, and the message, in `row`.
fn pages_row(ui: &mut egui::Ui, p: &Palette, row: Rect, page: &mut usize, count: usize, message: Option<&str>) {
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(row)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            if count > 1 {
                if widgets::icon_button(ui, p, "chevron-left", tr!("common-previous"), *page > 0) {
                    *page -= 1;
                }
                ui.label(
                    egui::RichText::new(tr!("fs-gesture-board-page", page = *page + 1, count = count))
                        .color(p.muted)
                        .small(),
                );
                if widgets::icon_button(ui, p, "chevron-right", tr!("common-next"), *page + 1 < count) {
                    *page += 1;
                }
            }
            if let Some(message) = message {
                ui.add(egui::Label::new(egui::RichText::new(message).color(p.warn).small()).truncate())
                    .on_hover_text(message);
            }
        },
    );
}

#[allow(clippy::too_many_arguments)]
fn search_results(
    ui: &mut egui::Ui,
    p: &Palette,
    account: &mut Account,
    facts: &Facts,
    view: &mut View,
    out: &mut Vec<Request>,
    full: Rect,
    without_footer: f32,
) {
    let Some(search) = view.search.as_ref() else {
        return;
    };
    let current = account.current_id();
    let hits = layout::search(account, &search.text, current, account.search_current, |item| {
        facts.inv.items.get(&item).map(|it| it.name.clone())
    });
    if search.text.trim().is_empty() {
        empty(ui, p, full, tr!("fs-gesture-board-search-empty"));
        return;
    }
    if hits.is_empty() {
        empty(ui, p, full, tr!("fs-gesture-board-search-none"));
        return;
    }
    let found: Vec<(&Board, &super::store::Tile, Item)> = hits
        .iter()
        .filter_map(|Hit { board, tile }| {
            let board = account.board(*board)?;
            let tile = board.tile(*tile)?;
            Some((board, tile, resolve(facts.inv, tile.item)))
        })
        .collect();
    let size_of = |ui: &egui::Ui, room: Vec2| -> Vec<Vec2> {
        found
            .iter()
            .map(|(board, tile, item)| {
                let size = tile.size_on(board);
                let sound = matches!(item, Item::Ready { kind: Kind::Sound, .. });
                let label = if board.full_names {
                    label_width(ui, &label_of(tile, item))
                } else {
                    0.0
                };
                vec2(layout::tile_width(board, size, label, sound, room.x), size[1].min(room.y))
            })
            .collect()
    };
    let room = vec2(full.width(), without_footer.max(1.0));
    let pages: Vec<Page> = layout::pages(&size_of(ui, room), room, 0);
    let count = pages.len();
    let mut page = search.page.min(count - 1);
    let room_rect = Rect::from_min_size(full.min, room);
    let mut chosen = None;
    for (index, rect) in &pages[page].tiles {
        let (board, tile, item) = &found[*index];
        let rect = rect.translate(room_rect.min.to_vec2());
        let response = ui.interact(rect, egui::Id::unique(("fs_gesture_board_hit", board.id, tile.id)), Sense::click());
        let (kind, original) = match item {
            Item::Ready { kind, original, .. } => (Some(*kind), Some(*original)),
            _ => (None, None),
        };
        let playing = kind == Some(Kind::Gesture) && original.is_some_and(|o| (facts.playing)(o));
        let blocked = if kind == Some(Kind::Gesture) {
            facts.gestures_blocked
        } else {
            None
        };
        let label = label_of(tile, item);
        paint_tile(
            ui,
            p,
            rect,
            &response,
            &Look {
                label: &label,
                kind,
                ready: original.is_some() && blocked.is_none(),
                color: tile.color,
                playing,
                outlined: false,
            },
        );
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, original.is_some(), &label));
        let other = (Some(board.id) != current).then_some(board.name.as_str());
        let response = response.on_hover_text(tooltip(tile, item, blocked, other));
        if response.clicked()
            && let (Some(original), Some(kind)) = (original, kind)
            && blocked.is_none()
        {
            out.push(Request::Play { original, kind });
        }
        if response.secondary_clicked() {
            chosen = Some((board.id, tile.id));
        }
    }
    let row = Rect::from_min_max(pos2(full.left(), room_rect.bottom() + ui.spacing().item_spacing.y), full.max);
    let summary = tr!("fs-gesture-board-found", count = hits.len());
    pages_row(ui, p, row, &mut page, count, None);
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(row)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
        |ui| {
            ui.label(egui::RichText::new(summary).color(p.muted).small());
        },
    );
    if let Some(search) = view.search.as_mut() {
        search.page = page;
    }
    // a right-click on a result shows it on its board
    if let Some((board, tile)) = chosen {
        account.selected = Some(board);
        view.search = None;
        view.flash(ui.ctx(), tile);
    }
}
