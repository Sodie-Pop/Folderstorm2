//! The switcher (folderstorm/plans/workspaces.md section 9.3; checkpoint 5
//! in detail, point 4): a window to change layout from the keyboard.
//!
//! Folderstorm (C++): "World → Workspaces & Layouts…, or add Workspaces to
//! a toolbar. Type part of a name and press Enter to switch. Favorites
//! appear first. Preview… shows the selected workspace before you apply
//! it. To keep your current arrangement, open Save, enter a new name,
//! choose what to capture, and click Save new" (doc/help.md "Workspaces").
//! Here it lists layouts; checkpoint 6 adds the workspaces.
//!
//! The search field has the keyboard when the window opens: ↑ ↓ select,
//! Enter applies the selected layout and closes the window, Esc closes it.
//! Those keys are read only while the field has the keyboard, when Aurora
//! sends no plain key to the world.
//!
//! Like the menu and the card, the window is drawn without the settings in
//! reach: it draws a copy of the store ([`Shown`]), published by the frame
//! call while the window is open, and leaves requests (`super::menu`) that
//! the next frame call runs.

use super::apply::{self, Parts};
use super::menu::{self, Request};
use super::pick::{self, Entry};
use super::preview;
use super::store::{Layout, MAX_LAYOUTS, MAX_NAME, Store, StoreError, WindowFrame, same_name};
use super::words;
use crate::folderstorm::viewport::ViewportSettings;
use crate::theme::Palette;
use crate::ui::widgets::{self, ActionKind, Floater};
use aurora_i18n::tr;
use egui::{Sense, Vec2, pos2, vec2};
use std::sync::Arc;

/// The window's id. Not on the allowlist (`super::windows`): a layout
/// never captures or moves it.
pub const WINDOW: &str = "fs-layouts-switcher";

/// The embedded Tabler icons of the window.
pub(super) const ICON_STAR: &str = "star-filled";
pub(super) const ICON_NO_STAR: &str = "star";
pub(super) const ICON_LOCK: &str = "lock-filled";
pub(super) const ICON_APPLY: &str = "check";
pub(super) const ICON_UPDATE: &str = "refresh";
pub(super) const ICON_SAVE: &str = "device-floppy-filled";
pub(super) const ICON_CANCEL: &str = "x";
pub(super) const ICON_PREFERENCES: &str = "settings-filled";

const ROW_H: f32 = 26.0;
const BUTTON_H: f32 = 26.0;
/// The list's width, in points; the picture takes the rest.
const LIST_W: f32 = 250.0;
const SIZE: Vec2 = vec2(640.0, 400.0);
/// A button's label shows at most this many characters of a name.
const SHORT_NAME: usize = 18;

/// What the window draws of the settings: a copy.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shown {
    pub store: Store,
    pub world: ViewportSettings,
}

/// *Save new…*, open in the window.
#[derive(Clone, Debug, Default, PartialEq)]
struct SaveForm {
    name: String,
    parts: Parts,
    /// The form was just opened: its field takes the keyboard.
    fresh: bool,
    /// Why the name was refused here, before a request was left.
    refused: Option<StoreError>,
}

/// The window's own state, for the session (egui's temporary data).
#[derive(Clone, Debug, Default, PartialEq)]
struct State {
    open: bool,
    /// Just opened: the field takes the keyboard.
    fresh: bool,
    /// What is typed in the search field.
    typed: String,
    /// The layout chosen with the arrows or a click, by its name. None:
    /// the first choice (`pick::first_choice`).
    chosen: Option<String>,
    save: Option<SaveForm>,
    /// *Update* has asked its question, for the layout of this name.
    ask_update: Option<String>,
}

fn state_id() -> egui::Id {
    egui::Id::unique("folderstorm_layouts_switcher")
}

fn shown_id() -> egui::Id {
    egui::Id::unique("folderstorm_layouts_switcher_shown")
}

pub(super) fn field_id() -> egui::Id {
    egui::Id::unique("folderstorm_layouts_switcher_field")
}

fn load(ctx: &egui::Context) -> State {
    ctx.data(|d| d.get_temp::<State>(state_id())).unwrap_or_default()
}

fn keep(ctx: &egui::Context, state: State) {
    ctx.data_mut(|d| d.insert_temp(state_id(), state));
}

/// Whether the switcher is open (its toolbar button is lit).
pub fn is_open(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<State>(state_id())).is_some_and(|state| state.open)
}

/// Opens the switcher as new: nothing typed, the field with the keyboard.
pub fn open(ctx: &egui::Context) {
    let mut open = false;
    widgets::open_floater(ctx, WINDOW, &mut open);
    keep(
        ctx,
        State {
            open,
            fresh: true,
            ..Default::default()
        },
    );
}

/// The switcher's toolbar button, menu entry and key: a window shown
/// closes, a minimized one expands, a closed one opens
/// (`widgets::toggle_floater`).
pub fn toggle(ctx: &egui::Context) {
    let mut state = load(ctx);
    let was = state.open;
    widgets::toggle_floater(ctx, WINDOW, &mut state.open);
    if state.open && !was {
        open(ctx);
    } else {
        keep(ctx, state);
    }
}

/// Keeps the window's copy of the store up to date while it is open, and
/// lets it go when it is closed (the frame call's part).
pub(super) fn publish(ctx: &egui::Context, store: &Store, world: &ViewportSettings) {
    if !is_open(ctx) {
        ctx.data_mut(|d| d.remove::<Arc<Shown>>(shown_id()));
        return;
    }
    let current = ctx.data(|d| d.get_temp::<Arc<Shown>>(shown_id()));
    if current.is_some_and(|shown| shown.store == *store && shown.world == *world) {
        return;
    }
    let shown = Arc::new(Shown {
        store: store.clone(),
        world: world.clone(),
    });
    ctx.data_mut(|d| d.insert_temp(shown_id(), shown));
}

fn shown(ctx: &egui::Context) -> Arc<Shown> {
    ctx.data(|d| d.get_temp::<Arc<Shown>>(shown_id())).unwrap_or_default()
}

/// A name for a button's label: cut after [`SHORT_NAME`] characters.
fn short(name: &str) -> String {
    if name.chars().count() <= SHORT_NAME {
        return name.to_owned();
    }
    let mut cut: String = name.chars().take(SHORT_NAME - 1).collect();
    cut.push('…');
    cut
}

fn muted(ui: &mut egui::Ui, p: &Palette, text: impl Into<String>) {
    ui.add(egui::Label::new(egui::RichText::new(text).size(11.5).color(p.muted)).wrap());
}

fn warning(ui: &mut egui::Ui, p: &Palette, text: impl Into<String>) {
    ui.add(egui::Label::new(egui::RichText::new(text).size(11.5).color(p.warn)).wrap());
}

fn refused(ui: &mut egui::Ui, p: &Palette, text: impl Into<String>) {
    ui.add(egui::Label::new(egui::RichText::new(text).size(11.5).color(p.danger)).wrap());
}

fn icon(ui: &egui::Ui, name: &str, center: egui::Pos2, side: f32, tint: egui::Color32) {
    if let Some(icon) = crate::ui::icons::global(name) {
        ui.painter().image(
            icon.id(),
            egui::Rect::from_center_size(center, Vec2::splat(side)),
            egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            tint,
        );
    }
}

/// What a click on a line asked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct LineAsk {
    select: bool,
    apply: bool,
    star: bool,
}

/// One line of the list: the star, the name, a lock for a layout that
/// locks the viewer window, "In use" or "In use · modified".
fn line(ui: &mut egui::Ui, p: &Palette, layout: &Layout, favorite: bool, selected: bool, note: &str) -> (LineAsk, egui::Response) {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
    let star_rect = egui::Rect::from_min_size(rect.min, vec2(28.0, rect.height()));
    let star = ui.interact(star_rect, ui.scope_id().with(("star", layout.name.as_str())), Sense::click());
    let fill = if selected {
        p.violet.gamma_multiply(0.35)
    } else if response.hovered() || star.hovered() {
        p.raised
    } else {
        p.field
    };
    ui.painter().rect_filled(rect, widgets::inner_radius(ui.visuals()), fill);
    let (star_icon, star_ink) = match (favorite, star.hovered()) {
        (true, _) => (ICON_STAR, p.warn),
        (false, true) => (ICON_NO_STAR, p.ink),
        (false, false) => (ICON_NO_STAR, p.muted_dim),
    };
    icon(ui, star_icon, star_rect.center(), 14.0, star_ink);
    let note_galley = ui
        .painter()
        .layout_no_wrap(note.to_owned(), egui::FontId::proportional(11.0), p.muted);
    let mut right = rect.right() - 8.0;
    if !note.is_empty() {
        right -= note_galley.size().x;
        ui.painter()
            .galley(pos2(right, rect.center().y - note_galley.size().y / 2.0), note_galley, p.muted);
        right -= 8.0;
    }
    let locked = layout.window.as_ref().is_some_and(WindowFrame::locked);
    if locked {
        right -= 7.0;
        icon(ui, ICON_LOCK, pos2(right, rect.center().y), 13.0, p.muted);
        right -= 13.0;
    }
    let left = star_rect.right() + 2.0;
    widgets::paint_elided(
        ui,
        &layout.name,
        pos2(left, rect.center().y),
        (right - left).max(1.0),
        egui::FontId::proportional(13.0),
        p.ink,
    );
    let star_tip = if favorite {
        tr!("fs-switcher-unstar")
    } else {
        tr!("fs-switcher-star")
    };
    let star = star.on_hover_text(star_tip);
    star.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, favorite, format!("{star_tip}: {}", layout.name)));
    let lock = if locked { tr!("fs-layouts-locked") } else { "" };
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, format!("{} {lock} {note}", layout.name)));
    let ask = LineAsk {
        select: response.clicked() && !star.clicked(),
        apply: response.double_clicked() && !star.hovered(),
        star: star.clicked(),
    };
    (ask, response)
}

fn heading(ui: &mut egui::Ui, p: &Palette, text: &str) {
    ui.add_space(2.0);
    ui.label(egui::RichText::new(text.to_uppercase()).size(10.5).color(p.muted));
}

/// The note at the right of a line.
fn note(shown: &Shown, index: usize) -> &'static str {
    if !shown.store.is_active(index) {
        ""
    } else if apply::modified(&shown.store, &shown.world) {
        tr!("fs-layouts-modified")
    } else {
        tr!("fs-layouts-active")
    }
}

/// The desktop of now, for the picture.
fn picture(ctx: &egui::Context, shown: &Shown, layout: &Layout) -> preview::Picture {
    let desktop = crate::folderstorm::diagram::desktop(ctx);
    let monitors = super::capture::monitors(ctx);
    preview::picture(
        layout,
        &preview::Now {
            monitors: &monitors,
            seen: apply::seen(ctx),
            window: desktop.window,
            position: desktop.position,
            pixels_per_point: ctx.pixels_per_point(),
            world: &shown.world,
        },
    )
}

/// What will not be as the layout has it, a line each.
pub(super) fn notes(notes: &preview::Notes) -> Vec<String> {
    let mut lines = Vec::new();
    if notes.missing > 0 {
        lines.push(tr!("fs-switcher-note-missing", count = notes.missing));
    }
    if notes.no_window {
        lines.push(tr!("fs-switcher-note-no-window").to_owned());
    }
    if notes.left {
        lines.push(tr!("fs-switcher-note-left").to_owned());
    }
    if notes.unlocked {
        lines.push(tr!("fs-switcher-note-unlocked").to_owned());
    }
    if notes.set_aside {
        lines.push(tr!("fs-switcher-note-set-aside").to_owned());
    }
    if notes.fitted > 0 {
        lines.push(tr!("fs-switcher-note-fitted", count = notes.fitted));
    }
    lines
}

/// The right side: the picture of the selected layout, what it holds, and
/// what will not be as saved.
fn side(ui: &mut egui::Ui, p: &Palette, shown: &Shown, layout: Option<&Layout>) {
    let Some(layout) = layout else {
        muted(
            ui,
            p,
            if shown.store.layouts.is_empty() {
                tr!("fs-switcher-empty")
            } else {
                tr!("fs-switcher-no-match")
            },
        );
        return;
    };
    let picture = picture(ui.ctx(), shown, layout);
    preview::show(ui, p, &picture).on_hover_text(tr!("fs-switcher-picture-help"));
    ui.add(egui::Label::new(egui::RichText::new(&layout.name).size(13.0).strong().color(p.ink)).truncate());
    muted(ui, p, words::holds(layout));
    for line in notes(&picture.notes) {
        warning(ui, p, line);
    }
}

/// What is wrong with saving under `name` now, known from the copy.
fn save_error(store: &Store, form: &SaveForm, readable: bool) -> Option<StoreError> {
    if !(form.parts.window && readable) && !form.parts.world && !form.parts.windows {
        return Some(StoreError::Invalid);
    }
    if let Some(error) = store.name_error(&form.name, None) {
        return Some(StoreError::Name(error));
    }
    (store.layouts.len() >= MAX_LAYOUTS).then_some(StoreError::Full)
}

/// *Save new…*: the name, the parts, Save and Cancel. True when the form
/// is to be closed.
fn save_form(ui: &mut egui::Ui, p: &Palette, shown: &Shown, form: &mut SaveForm, out: &mut Vec<Request>) -> bool {
    let mut close = false;
    let mut save = false;
    // (no viewer window to read under a forced size or a capture)
    let readable = apply::has_viewer_window(ui.ctx());
    ui.horizontal(|ui| {
        let field = ui.add(
            egui::TextEdit::singleline(&mut form.name)
                .desired_width(220.0)
                .char_limit(MAX_NAME * 2)
                .hint_text(tr!("fs-layouts-name-hint")),
        );
        if std::mem::take(&mut form.fresh) {
            field.request_focus();
        }
        if field.changed() {
            form.refused = None;
        }
        save |= field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        close |= field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape));
        save |= widgets::action_button(ui, p, ICON_SAVE, tr!("fs-layouts-save"), ActionKind::Primary, vec2(0.0, BUTTON_H)).clicked();
        close |= widgets::action_button(ui, p, ICON_CANCEL, tr!("common-cancel"), ActionKind::Flat, vec2(0.0, BUTTON_H)).clicked();
    });
    ui.horizontal_wrapped(|ui| {
        ui.add_enabled_ui(readable, |ui| {
            let mut window = form.parts.window && readable;
            if widgets::checkbox(ui, p, &mut window, tr!("fs-layouts-part-window")).changed() {
                form.parts.window = window;
            }
        });
        widgets::checkbox(ui, p, &mut form.parts.world, tr!("fs-layouts-part-world"));
        widgets::checkbox(ui, p, &mut form.parts.windows, tr!("fs-layouts-part-windows"));
        let with_window = form.parts.window && readable;
        ui.add_enabled_ui(with_window, |ui| {
            let mut lock = form.parts.lock && with_window;
            if widgets::checkbox(ui, p, &mut lock, tr!("fs-layouts-lock")).changed() {
                form.parts.lock = lock;
            }
        });
    });
    if save {
        form.refused = save_error(&shown.store, form, readable);
        if form.refused.is_none() {
            out.push(Request::SaveNew(form.name.clone(), form.parts));
            close = true;
        }
    }
    match form.refused {
        // (the parts, not the arrangement: the tab's sentence for it)
        Some(StoreError::Invalid) => refused(ui, p, tr!("fs-layouts-error-parts")),
        Some(error) => refused(ui, p, words::refusal(error)),
        None => {}
    }
    close
}

/// The window's footer: the question of *Update*, or *Save new…*, or the
/// buttons; and one line for what was last done.
fn footer(ui: &mut egui::Ui, p: &Palette, shown: &Shown, st: &mut State, selected: Option<&Layout>, out: &mut Vec<Request>) {
    let ctx = ui.ctx().clone();
    let size = vec2(0.0, BUTTON_H);
    ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
    widgets::divider(ui, p);
    if let Some(name) = st.ask_update.clone() {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                egui::RichText::new(tr!("fs-layouts-ask-update", name = name.as_str()))
                    .size(12.5)
                    .color(p.ink),
            );
            if widgets::action_button(ui, p, ICON_UPDATE, tr!("fs-layouts-update"), ActionKind::Primary, size).clicked() {
                out.push(Request::Update(name.clone()));
                st.ask_update = None;
            }
            if widgets::action_button(ui, p, ICON_CANCEL, tr!("common-cancel"), ActionKind::Flat, size).clicked() {
                st.ask_update = None;
            }
        });
        return;
    }
    if let Some(form) = &mut st.save {
        if save_form(ui, p, shown, form, out) {
            st.save = None;
        }
        return;
    }
    let active = shown.store.active().map(|layout| layout.name.clone());
    ui.horizontal_wrapped(|ui| {
        if widgets::action_button_if(
            ui,
            p,
            ICON_APPLY,
            tr!("fs-layouts-apply"),
            ActionKind::Primary,
            size,
            selected.is_some(),
        )
        .clicked()
            && let Some(layout) = selected
        {
            out.push(Request::Apply(layout.name.clone()));
        }
        // the layout in use takes the arrangement of now (section 8.1)
        let update = match &active {
            Some(name) => tr!("fs-switcher-update", name = short(name)),
            None => tr!("fs-layouts-update").to_owned(),
        };
        let asked = widgets::action_button_if(ui, p, ICON_UPDATE, &update, ActionKind::Flat, size, active.is_some())
            .on_hover_text(tr!("fs-switcher-update-help"))
            .clicked();
        if asked {
            st.ask_update = active.clone();
        }
        if widgets::action_button(ui, p, ICON_SAVE, tr!("fs-switcher-save-new"), ActionKind::Flat, size)
            .on_hover_text(tr!("fs-layouts-save-new-help"))
            .clicked()
        {
            st.save = Some(SaveForm {
                fresh: true,
                ..Default::default()
            });
        }
        let back = apply::has_previous(&ctx);
        if widgets::action_button_if(ui, p, menu::ICON_PREVIOUS, tr!("fs-layouts-previous"), ActionKind::Flat, size, back)
            .on_hover_text(tr!("fs-layouts-previous-help"))
            .clicked()
        {
            out.push(Request::Previous);
        }
        if shown.store.window_held
            && widgets::action_button(ui, p, menu::ICON_UNLOCK, tr!("fs-layouts-unlock-button"), ActionKind::Flat, size)
                .on_hover_text(tr!("fs-layouts-unlock-help"))
                .clicked()
        {
            out.push(Request::Unlock);
        }
        if widgets::action_button(ui, p, ICON_PREFERENCES, tr!("common-preferences"), ActionKind::Flat, size)
            .on_hover_text(tr!("fs-switcher-preferences-help"))
            .clicked()
        {
            out.push(Request::Preferences);
        }
    });
    if let Some(error) = apply::refused(&ctx) {
        refused(ui, p, words::refusal(error));
    } else if let Some(report) = apply::report(&ctx) {
        muted(ui, p, words::report_text(&report, shown.store.pending.len()));
    } else {
        muted(ui, p, tr!("fs-switcher-keys"));
    }
}

/// The name of the layout of a line.
fn name_at<'a>(store: &'a Store, lines: &[Entry], at: Option<usize>) -> Option<&'a Layout> {
    at.and_then(|at| lines.get(at)).and_then(|line| store.layout(line.index))
}

/// The window's body. True when it is to be closed.
fn body(ui: &mut egui::Ui, p: &Palette, shown: &Shown, st: &mut State, out: &mut Vec<Request>) -> bool {
    let store = &shown.store;
    let mut close = false;
    let lines = pick::filtered(store, &st.typed);
    let place = |chosen: &Option<String>| {
        chosen
            .as_deref()
            .and_then(|name| {
                lines
                    .iter()
                    .position(|line| store.layout(line.index).is_some_and(|layout| same_name(&layout.name, name)))
            })
            .or_else(|| pick::first_choice(store, &lines, &st.typed))
    };
    let mut at = place(&st.chosen);
    // the list's keys, read before the field (which would lose the
    // keyboard on Enter) and only while it has it
    let field = field_id();
    let mut reveal = false;
    if ui.memory(|m| m.has_focus(field)) && st.save.is_none() && st.ask_update.is_none() {
        let none = egui::Modifiers::NONE;
        let (down, up, enter) = ui.input_mut(|i| {
            (
                i.consume_key(none, egui::Key::ArrowDown),
                i.consume_key(none, egui::Key::ArrowUp),
                i.consume_key(none, egui::Key::Enter),
            )
        });
        if down || up {
            at = pick::step(lines.len(), at, down);
            st.chosen = name_at(store, &lines, at).map(|layout| layout.name.clone());
            reveal = true;
        }
        if down || up || enter {
            // the keyboard stays in the field: the arrows do not walk the
            // window's widgets
            ui.memory_mut(|m| {
                m.request_focus(field);
                m.move_focus(egui::FocusDirection::None);
            });
        }
        if enter && let Some(layout) = name_at(store, &lines, at) {
            out.push(Request::Apply(layout.name.clone()));
            close = true;
        }
    }
    let width = ui.available_width();
    let typed_before = st.typed.clone();
    let response = widgets::search_text_edit(ui, p, &mut st.typed, |edit| {
        edit.id(field)
            .hint_text(tr!("fs-switcher-hint"))
            .desired_width(width)
            .char_limit(MAX_NAME * 2)
    });
    if std::mem::take(&mut st.fresh) {
        response.request_focus();
    }
    if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        close = true;
    }
    if st.typed != typed_before {
        // what is typed chooses anew: the first that answers it
        st.chosen = None;
        reveal = true;
    }
    let lines = if st.typed != typed_before {
        pick::filtered(store, &st.typed)
    } else {
        lines
    };
    let at = if st.typed != typed_before {
        pick::first_choice(store, &lines, &st.typed)
    } else {
        at
    };
    let selected = name_at(store, &lines, at);
    ui.add_space(4.0);
    let (mut above, foot) = widgets::above_footer(ui);
    let mut clicked = None;
    above.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(LIST_W);
            ui.spacing_mut().item_spacing.y = 3.0;
            egui::ScrollArea::vertical()
                .id_salt("fs-switcher-list")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let headings = lines.iter().any(|line| line.favorite);
                    let mut section = None;
                    for (n, entry) in lines.iter().enumerate() {
                        let Some(layout) = store.layout(entry.index) else { continue };
                        if headings && section != Some(entry.favorite) {
                            section = Some(entry.favorite);
                            let title = if entry.favorite {
                                tr!("fs-switcher-favorites")
                            } else {
                                tr!("fs-switcher-layouts")
                            };
                            heading(ui, p, title);
                        }
                        let (ask, response) = line(ui, p, layout, entry.favorite, at == Some(n), note(shown, entry.index));
                        if reveal && at == Some(n) {
                            response.scroll_to_me(None);
                        }
                        if ask.star {
                            out.push(Request::Star(layout.name.clone(), !entry.favorite));
                        }
                        if ask.select || ask.apply {
                            clicked = Some(layout.name.clone());
                        }
                        if ask.apply {
                            out.push(Request::Apply(layout.name.clone()));
                        }
                    }
                });
        });
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            side(ui, p, shown, selected);
        });
    });
    if let Some(name) = clicked {
        st.chosen = Some(name);
        // the keyboard goes back to the field: the arrows and Enter carry on
        // from the line clicked
        ui.memory_mut(|m| m.request_focus(field));
    }
    foot.show(ui, |ui| footer(ui, p, shown, st, selected, out));
    close
}

/// Draws the switcher when it is open (from the interface's pass).
pub fn show(ctx: &egui::Context, p: &Palette) {
    let mut st = load(ctx);
    if !st.open {
        return;
    }
    let before = st.clone();
    let shown = shown(ctx);
    let room = crate::ui::layout::work_area(ctx);
    let size = SIZE.min(room.size() - Vec2::splat(24.0)).max(vec2(360.0, 260.0));
    let pos = pos2(room.center().x - size.x / 2.0, room.top() + (room.height() - size.y) * 0.3);
    let mut open = true;
    let mut out = Vec::new();
    let close = Floater::new(WINDOW, tr!("fs-switcher-title"), pos, size)
        .help(tr!("fs-switcher-help"))
        .show(ctx, p, &mut open, |ui| body(ui, p, &shown, &mut st, &mut out))
        .unwrap_or(false);
    if close || !open {
        st.open = false;
    }
    for request in out {
        menu::ask(ctx, request);
    }
    if st != before {
        keep(ctx, st);
    }
}
