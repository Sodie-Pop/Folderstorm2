//! The board window's dialogs: a tile's editor (label, colour, size), the
//! Add picker, a board's name (new, rename, duplicate), its deletion and
//! its settings (the size of all its tiles, full names, tiles per page).
//! Editing a tile is staged until Save (Folderstorm (C++): "Save commits,
//! Cancel leaves it unchanged"); the board's settings apply as they are
//! moved, so that the tiles can be watched while they resize.

use super::layout;
use super::store::{self, Account, Board};
use super::window::{Facts, View};
use super::{Item, Kind, resolve};
use crate::theme::Palette;
use crate::ui::widgets::{self, ActionKind, Modal, action_button, action_button_if};
use crate::world::inventory::actions as rules;
use aurora_i18n::tr;
use egui::{Color32, RichText, Sense, Stroke, Vec2, vec2};
use std::collections::BTreeSet;
use uuid::Uuid;

#[derive(Clone)]
pub(super) enum Dialog {
    /// A board's name: a new board, a rename, a copy.
    Name { purpose: Purpose, text: String, select: bool },
    /// Delete this board?
    Delete(u32),
    /// The board's settings.
    Settings(u32),
    /// A tile's editor.
    Edit(Box<Editor>),
    /// Add gestures and sounds.
    Add(Box<Picker>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Purpose {
    New,
    Rename(u32),
    Duplicate(u32),
}

#[derive(Clone)]
pub(super) struct Editor {
    board: u32,
    tile: u32,
    /// Empty: the item's name.
    label: String,
    color: Option<[u8; 3]>,
    own_size: bool,
    size: [f32; 2],
}

#[derive(Clone, Default)]
pub(super) struct Picker {
    /// 0 all, 1 gestures, 2 sounds.
    kind: usize,
    filter: String,
    chosen: BTreeSet<Uuid>,
    /// The user's gestures and sounds (name, kind), for this inventory
    /// generation.
    rows: Vec<(Uuid, String, Kind)>,
    generation: Option<u64>,
}

impl Dialog {
    pub(super) fn new_board() -> Self {
        Dialog::Name {
            purpose: Purpose::New,
            text: tr!("fs-gesture-board-new-name").to_owned(),
            select: true,
        }
    }

    pub(super) fn rename(id: u32, name: &str) -> Self {
        Dialog::Name {
            purpose: Purpose::Rename(id),
            text: name.to_owned(),
            select: true,
        }
    }

    pub(super) fn duplicate(id: u32, name: &str) -> Self {
        Dialog::Name {
            purpose: Purpose::Duplicate(id),
            text: store::clean_name(name),
            select: true,
        }
    }

    pub(super) fn add() -> Self {
        Dialog::Add(Box::default())
    }

    /// The editor of the tile `tile` of `board`, as it is now.
    pub(super) fn edit(board: &Board, tile: u32) -> Option<Self> {
        let t = board.tile(tile)?;
        Some(Dialog::Edit(Box::new(Editor {
            board: board.id,
            tile,
            label: t.label.clone().unwrap_or_default(),
            color: t.color,
            own_size: t.size.is_some(),
            size: t.size_on(board),
        })))
    }
}

/// Draws the dialog open over the window, if any.
pub(super) fn show(ctx: &egui::Context, p: &Palette, account: &mut Account, facts: &Facts, view: &mut View) {
    let Some(mut dialog) = view.dialog.take() else {
        return;
    };
    let keep = match &mut dialog {
        Dialog::Name { purpose, text, select } => name(ctx, p, account, view, *purpose, text, select),
        Dialog::Delete(id) => delete(ctx, p, account, *id),
        Dialog::Settings(id) => settings(ctx, p, account, *id),
        Dialog::Edit(editor) => edit(ctx, p, account, facts, editor),
        Dialog::Add(picker) => add(ctx, p, account, facts, view, picker),
    };
    // (a dialog may have opened another, or the search)
    if keep && view.dialog.is_none() {
        view.dialog = Some(dialog);
    }
}

fn title(ui: &mut egui::Ui, p: &Palette, text: &str) {
    ui.label(RichText::new(text).size(17.0).strong().color(p.ink));
    ui.add_space(6.0);
}

const BUTTON: Vec2 = Vec2::new(0.0, 26.0);

/// Ok / Cancel; (ok, cancel).
fn buttons(ui: &mut egui::Ui, p: &Palette, ok: &str, valid: bool) -> (bool, bool) {
    let mut out = (false, false);
    ui.horizontal(|ui| {
        let enter = widgets::enter_pressed(ui);
        out.0 = action_button_if(ui, p, "check", ok, ActionKind::Primary, BUTTON, valid).clicked() || (valid && enter);
        out.1 = action_button(ui, p, "x", tr!("common-cancel"), ActionKind::Flat, BUTTON).clicked();
    });
    out
}

fn name(
    ctx: &egui::Context,
    p: &Palette,
    account: &mut Account,
    view: &mut View,
    purpose: Purpose,
    text: &mut String,
    select: &mut bool,
) -> bool {
    let mut close = false;
    let response = Modal::new(egui::Id::unique("fs_gesture_board_name")).show(ctx, |ui| {
        ui.set_width(340.0);
        title(
            ui,
            p,
            match purpose {
                Purpose::New => tr!("fs-gesture-board-new"),
                Purpose::Rename(_) => tr!("fs-gesture-board-rename"),
                Purpose::Duplicate(_) => tr!("fs-gesture-board-duplicate"),
            },
        );
        let field = ui.add(
            egui::TextEdit::singleline(text)
                .desired_width(f32::INFINITY)
                .char_limit(store::MAX_NAME)
                .hint_text(tr!("fs-gesture-board-name-hint")),
        );
        if *select {
            field.request_focus();
            if let Some(mut state) = egui::TextEdit::load_state(ctx, field.id) {
                state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
                    egui::text::CCursor::new(0),
                    egui::text::CCursor::new(text.chars().count()),
                )));
                state.store(ctx, field.id);
            }
            *select = false;
        }
        ui.add_space(8.0);
        let valid = !store::clean_name(text).is_empty();
        let (ok, cancel) = buttons(ui, p, tr!("common-ok"), valid);
        if ok {
            let done = match purpose {
                Purpose::New => account.new_board(text).map(|_| ()),
                Purpose::Rename(id) => account.rename_board(id, text),
                Purpose::Duplicate(id) => account.duplicate_board(id, text).map(|_| ()),
            };
            if let Err(error) = done {
                view.say(ctx, error.text());
            }
            close = true;
        }
        close |= cancel;
    });
    !(close || response.should_close())
}

fn delete(ctx: &egui::Context, p: &Palette, account: &mut Account, id: u32) -> bool {
    let Some(board) = account.board(id) else {
        return false;
    };
    let name = board.name.clone();
    let count = board.tiles.len();
    let mut close = false;
    let response = Modal::new(egui::Id::unique("fs_gesture_board_delete")).show(ctx, |ui| {
        ui.set_width(340.0);
        title(ui, p, tr!("fs-gesture-board-delete"));
        ui.label(tr!("fs-gesture-board-delete-question", name = name.as_str(), count = count));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if action_button(ui, p, "trash-filled", tr!("common-delete"), ActionKind::Danger, BUTTON).clicked() {
                let _ = account.delete_board(id);
                close = true;
            }
            close |= action_button(ui, p, "x", tr!("common-cancel"), ActionKind::Flat, BUTTON).clicked();
        });
    });
    !(close || response.should_close())
}

fn size_sliders(ui: &mut egui::Ui, size: &mut [f32; 2]) -> bool {
    let mut changed = false;
    egui::Grid::new(ui.next_auto_id()).num_columns(2).show(ui, |ui| {
        ui.label(tr!("fs-gesture-board-width"));
        changed |= ui
            .add(egui::Slider::new(&mut size[0], store::WIDTHS).step_by(2.0).suffix(" pt"))
            .changed();
        ui.end_row();
        ui.label(tr!("fs-gesture-board-height"));
        changed |= ui
            .add(egui::Slider::new(&mut size[1], store::HEIGHTS).step_by(2.0).suffix(" pt"))
            .changed();
        ui.end_row();
    });
    changed
}

fn settings(ctx: &egui::Context, p: &Palette, account: &mut Account, id: u32) -> bool {
    let Some(board) = account.board_mut(id) else {
        return false;
    };
    let mut close = false;
    let response = Modal::new(egui::Id::unique("fs_gesture_board_settings")).show(ctx, |ui| {
        ui.set_width(360.0);
        title(ui, p, tr!("fs-gesture-board-settings"));
        ui.label(RichText::new(tr!("fs-gesture-board-size-all")).color(p.muted));
        size_sliders(ui, &mut board.tile);
        let own = board.tiles.iter().filter(|t| t.size.is_some()).count();
        if action_button_if(
            ui,
            p,
            "dimensions",
            &tr!("fs-gesture-board-same-size", count = own),
            ActionKind::Flat,
            BUTTON,
            own > 0,
        )
        .clicked()
        {
            board.same_size_for_all();
        }
        ui.add_space(6.0);
        widgets::checkbox(ui, p, &mut board.full_names, tr!("fs-gesture-board-full-names"));
        ui.label(RichText::new(tr!("fs-gesture-board-full-names-help")).color(p.muted).small());
        ui.add_space(6.0);
        let mut auto = board.per_page == 0;
        if widgets::checkbox(ui, p, &mut auto, tr!("fs-gesture-board-per-page-auto")).changed() {
            board.per_page = if auto { 0 } else { 24 };
        }
        if !auto {
            let mut n = board.per_page;
            ui.horizontal(|ui| {
                ui.label(tr!("fs-gesture-board-per-page"));
                if ui.add(egui::Slider::new(&mut n, store::PER_PAGE)).changed() {
                    board.per_page = store::clamp_per_page(n);
                }
            });
        }
        ui.add_space(8.0);
        close = action_button(ui, p, "check", tr!("common-close"), ActionKind::Primary, BUTTON).clicked();
    });
    !(close || response.should_close())
}

/// Swatches taken from the palette (so that they suit the skin), then
/// any colour with the colour picker.
fn swatches(p: &Palette) -> [Color32; 8] {
    [p.violet, p.indigo, p.teal, p.success, p.amber, p.warn, p.rose, p.danger]
}

fn rgb(c: Color32) -> [u8; 3] {
    let c = c.to_opaque();
    [c.r(), c.g(), c.b()]
}

fn edit(ctx: &egui::Context, p: &Palette, account: &mut Account, facts: &Facts, editor: &mut Editor) -> bool {
    let Some(board) = account.board(editor.board) else {
        return false;
    };
    let Some(tile) = board.tile(editor.tile) else {
        return false;
    };
    let item = resolve(facts.inv, tile.item);
    let item_name = super::window::label_of(
        &store::Tile {
            label: None,
            ..tile.clone()
        },
        &item,
    );
    let board_size = board.tile;
    let mut close = false;
    let mut save = false;
    let response = Modal::new(egui::Id::unique("fs_gesture_board_edit")).show(ctx, |ui| {
        ui.set_width(380.0);
        title(ui, p, tr!("fs-gesture-board-edit-title"));
        ui.label(RichText::new(&item_name).color(p.muted));
        if let Some(why) = item.why() {
            ui.label(RichText::new(why).color(p.warn).small());
        }
        ui.add_space(4.0);
        ui.label(tr!("fs-gesture-board-label"));
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut editor.label)
                    .desired_width(ui.available_width() - 28.0)
                    .char_limit(store::MAX_NAME)
                    .hint_text(&item_name),
            );
            if widgets::icon_button(ui, p, "x", tr!("fs-gesture-board-label-reset"), !editor.label.is_empty()) {
                editor.label.clear();
            }
        });
        ui.add_space(6.0);
        ui.label(tr!("common-color"));
        ui.horizontal_wrapped(|ui| {
            if widgets::radio(ui, p, editor.color.is_none(), tr!("fs-gesture-board-theme-color")).clicked() {
                editor.color = None;
            }
            for color in swatches(p) {
                let (rect, response) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
                let radius = ui.visuals().widgets.inactive.corner_radius;
                ui.painter().rect_filled(rect, radius, color.to_opaque());
                if editor.color == Some(rgb(color)) {
                    ui.painter()
                        .rect_stroke(rect, radius, Stroke::new(2.0, p.ink), egui::StrokeKind::Outside);
                }
                if response.clicked() {
                    editor.color = Some(rgb(color));
                }
            }
            let [r, g, b] = editor.color.unwrap_or_else(|| rgb(p.raised));
            let mut rgba = [r, g, b, 255];
            if widgets::color_button(ui, p, "fs_gesture_board_color", &mut rgba, false)
                .on_hover_text(tr!("fs-gesture-board-custom-color"))
                .changed()
            {
                editor.color = Some([rgba[0], rgba[1], rgba[2]]);
            }
        });
        ui.add_space(6.0);
        widgets::checkbox(ui, p, &mut editor.own_size, tr!("fs-gesture-board-own-size"));
        if editor.own_size {
            size_sliders(ui, &mut editor.size);
        } else {
            editor.size = board_size;
        }
        ui.add_space(6.0);
        // the tile as it will look (never played from here)
        let label = if editor.label.trim().is_empty() {
            item_name.clone()
        } else {
            editor.label.clone()
        };
        let shown = vec2(editor.size[0].min(ui.available_width()), editor.size[1]);
        let (rect, response) = ui.allocate_exact_size(shown, Sense::hover());
        let kind = match &item {
            Item::Ready { kind, .. } => Some(*kind),
            _ => None,
        };
        preview(ui, p, rect, &response, &label, kind, editor.color);
        ui.add_space(8.0);
        let (ok, cancel) = buttons(ui, p, tr!("common-save"), true);
        save = ok;
        close = ok || cancel;
    });
    if save && let Some(tile) = account.board_mut(editor.board).and_then(|b| b.tile_mut(editor.tile)) {
        let label = store::clean_name(&editor.label);
        tile.label = (!label.is_empty() && label != item_name).then_some(label);
        tile.color = editor.color;
        tile.size = editor.own_size.then(|| store::clamp_size(editor.size));
    }
    !(close || response.should_close())
}

fn preview(
    ui: &egui::Ui,
    p: &Palette,
    rect: egui::Rect,
    response: &egui::Response,
    label: &str,
    kind: Option<Kind>,
    color: Option<[u8; 3]>,
) {
    let radius = ui.visuals().widgets.inactive.corner_radius;
    let fill = match color {
        // theme-guard: content — the colour the user chose for this tile
        Some([r, g, b]) => Color32::from_rgb(r, g, b),
        None => ui.style().interact(response).weak_bg_fill,
    };
    ui.painter().rect_filled(rect, radius, fill);
    ui.painter()
        .rect_stroke(rect, radius, Stroke::new(1.0, p.muted_dim), egui::StrokeKind::Inside);
    let ink = super::window::ink_on(p, fill);
    let speaker = if kind == Some(Kind::Sound) { layout::SPEAKER } else { 0.0 };
    if speaker > 0.0 {
        widgets::paint_glyph(
            ui.painter(),
            super::SOUND_ICON,
            egui::pos2(rect.right() - layout::PAD - speaker / 2.0 + 2.0, rect.center().y),
            12.0,
            ink,
        );
    }
    let width = (rect.width() - 2.0 * layout::PAD - speaker).max(4.0);
    let mut job = egui::text::LayoutJob::single_section(
        label.to_owned(),
        egui::TextFormat::simple(egui::TextStyle::Button.resolve(ui.style()), ink),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(width);
    let galley = ui.painter().layout_job(job);
    let at = egui::pos2(
        rect.left() + layout::PAD + (width - galley.size().x).max(0.0) / 2.0,
        rect.center().y - galley.size().y / 2.0,
    );
    ui.painter().with_clip_rect(rect).galley(at, galley, ink);
}

/// The user's gestures and sounds that a board may hold: of their own
/// inventory, not in the Trash, not links (their originals are listed).
pub(super) fn catalogue(inv: &crate::world::inventory::Inventory) -> Vec<(Uuid, String, Kind)> {
    let mut rows: Vec<_> = inv
        .items
        .values()
        .filter(|it| matches!(it.asset_type, 1 | 21))
        .filter(|it| !rules::library(inv, it.id) && !rules::in_type(inv, it.id, 14))
        .map(|it| {
            (
                it.id,
                it.name.clone(),
                if it.asset_type == 21 { Kind::Gesture } else { Kind::Sound },
            )
        })
        .collect();
    rows.sort_by_cached_key(|(id, name, _)| (crate::natural_order::key(name), *id));
    rows
}

fn add(ctx: &egui::Context, p: &Palette, account: &mut Account, facts: &Facts, view: &mut View, picker: &mut Picker) -> bool {
    let Some(board_id) = account.current_id() else {
        return false;
    };
    if picker.generation != Some(facts.inv.generation) {
        picker.generation = Some(facts.inv.generation);
        picker.rows = catalogue(facts.inv);
        picker.chosen.retain(|id| facts.inv.items.contains_key(id));
    }
    let on_board: BTreeSet<Uuid> = account
        .board(board_id)
        .map(|b| b.tiles.iter().map(|t| t.item).collect())
        .unwrap_or_default();
    let mut close = false;
    let mut chosen_now = None;
    let response = Modal::new(egui::Id::unique("fs_gesture_board_add")).show(ctx, |ui| {
        ui.set_width(400.0);
        title(ui, p, tr!("fs-gesture-board-add"));
        widgets::segmented(
            ui,
            p,
            &mut picker.kind,
            &[
                (tr!("common-all"), true),
                (tr!("fs-gesture-board-gestures"), true),
                (tr!("common-sounds"), true),
            ],
        );
        ui.add_space(4.0);
        let width = ui.available_width();
        widgets::search_text_edit(ui, p, &mut picker.filter, |edit| {
            edit.hint_text(tr!("fs-gesture-board-filter")).desired_width(width - 8.0)
        });
        ui.add_space(4.0);
        let filter = picker.filter.trim().to_lowercase();
        let rows: Vec<&(Uuid, String, Kind)> = picker
            .rows
            .iter()
            .filter(|(_, name, kind)| {
                (picker.kind == 0 || (picker.kind == 1) == (*kind == Kind::Gesture))
                    && (filter.is_empty() || name.to_lowercase().contains(&filter))
            })
            .collect();
        let row_height = 22.0;
        egui::ScrollArea::vertical()
            .max_height(260.0)
            .auto_shrink([false, false])
            .show_rows(ui, row_height, rows.len(), |ui, range| {
                for (id, name, kind) in &rows[range] {
                    let there = on_board.contains(id);
                    ui.horizontal(|ui| {
                        ui.set_height(row_height);
                        let mut on = there || picker.chosen.contains(id);
                        ui.add_enabled_ui(!there, |ui| {
                            if widgets::checkbox(ui, p, &mut on, "").changed() {
                                if on {
                                    picker.chosen.insert(*id);
                                } else {
                                    picker.chosen.remove(id);
                                }
                            }
                        });
                        let icon = if *kind == Kind::Sound { super::SOUND_ICON } else { "hand-move" };
                        widgets::glyph(ui, icon, if there { p.muted_dim } else { p.muted }, 14.0);
                        let text = RichText::new(name).color(if there { p.muted_dim } else { p.ink });
                        let label = ui.add(egui::Label::new(text).truncate().sense(Sense::click()));
                        if there {
                            label.on_hover_text(tr!("fs-gesture-board-on-this-board"));
                        } else if label.clicked() && !picker.chosen.remove(id) {
                            picker.chosen.insert(*id);
                        }
                    });
                }
            });
        if rows.is_empty() {
            ui.label(RichText::new(tr!("fs-gesture-board-add-none")).color(p.muted));
        }
        ui.add_space(8.0);
        let count = picker.chosen.len();
        let (ok, cancel) = buttons(ui, p, &tr!("fs-gesture-board-add-count", count = count), count > 0);
        if ok {
            let order: Vec<Uuid> = picker
                .rows
                .iter()
                .filter(|(id, _, _)| picker.chosen.contains(id))
                .map(|(id, _, _)| *id)
                .collect();
            chosen_now = Some(order);
        }
        close = ok || cancel;
    });
    if let Some(items) = chosen_now {
        match account.add_items(board_id, &items, None) {
            Ok(added) => {
                if let Some(tile) = added.added.first() {
                    view.flash(ctx, *tile);
                }
                if added.refused > 0 {
                    view.say(ctx, store::Error::TooManyTiles.text());
                }
            }
            Err(error) => view.say(ctx, error.text()),
        }
    }
    !(close || response.should_close())
}
