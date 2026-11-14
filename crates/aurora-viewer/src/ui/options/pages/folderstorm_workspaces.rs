//! Folderstorm: Preferences › Layout › Layouts & workspaces
//! (`crate::folderstorm::workspaces`, FEATURES.md row 1.3). Folderstorm (C++)
//! saved its bar placement as named profiles under Move & View › Layout:
//! apply, rename, delete, Reset.
//!
//! The list of saved layouts, the actions on the selected one (Apply,
//! Update, Rename, Delete, Move up and down) and its viewer window (mode,
//! monitors, lock), *Save as a new layout* with its parts, *Previous
//! arrangement*, *Unlock the viewer window*, *When the monitors change*
//! and *Reset bars and world*; a layout's star, and the two layouts of the
//! toolbar's toggle (*Quick switch*). Like the rest of Preferences, everything
//! applies at once; Update and Delete ask first, in the page. The rows
//! edit no tracked setting: "Reset this page" never deletes a layout.
//! The workspaces' groups are in [`workspaces_group`].

mod workspaces_group;

use super::super::cx::{Cx, Nothing};
use crate::folderstorm::workspaces::apply::{self, Live, Parts};
use crate::folderstorm::workspaces::monitors;
use crate::folderstorm::workspaces::store::{Layout, MAX_NAME, Monitor, MonitorPolicy, Record, Store, StoreError, Unusable, WindowFrame};
use crate::folderstorm::workspaces::words::{self, refusal, report_text};
use crate::settings::Settings;
use crate::theme::Palette;
use crate::ui::widgets::{self, ActionKind};
use aurora_i18n::tr;
use egui::{Sense, Vec2, pos2, vec2};

/// The embedded Tabler icons this tab uses.
const ICON_CHECK: &str = "check";
const ICON_SAVE: &str = "device-floppy-filled";
const ICON_RENAME: &str = "pencil-filled";
const ICON_DELETE: &str = "trash-filled";
const ICON_CANCEL: &str = "x";
const ICON_BACK: &str = "arrow-back-up";
const ICON_RESET: &str = "restore";
const ICON_UP: &str = "arrow-up";
const ICON_DOWN: &str = "arrow-down";
const ICON_LOCK: &str = "lock-filled";
const ICON_UNLOCK: &str = "lock-open";
const ICON_STAR: &str = "star-filled";

const ROW_H: f32 = 28.0;
const BUTTON_H: f32 = 26.0;
const LIST_W: f32 = 560.0;
/// The width of a list of layouts to pick from.
const PICKER_W: f32 = 220.0;

/// A question asked in the page before a change that loses something.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ask {
    Update,
    Delete,
}

/// What was last refused, to say why under the field it came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Refused {
    Save(StoreError),
    Rename(StoreError),
    Selected(StoreError),
}

/// The tab's own state, for the session (egui's temporary data).
#[derive(Clone, Debug, Default)]
struct Tab {
    /// The entry selected in the list.
    selected: Option<usize>,
    /// The name typed for a new layout, and its parts.
    new_name: String,
    parts: Parts,
    /// The name being typed for the selected layout.
    rename: Option<String>,
    ask: Option<Ask>,
    refused: Option<Refused>,
    /// *Previous arrangement* was the last thing done here.
    went_back: bool,
}

fn tab_id() -> egui::Id {
    egui::Id::unique("folderstorm_layouts_tab")
}

fn load(ctx: &egui::Context) -> Tab {
    ctx.data(|d| d.get_temp::<Tab>(tab_id())).unwrap_or_default()
}

fn store(ctx: &egui::Context, tab: Tab) {
    ctx.data_mut(|d| d.insert_temp(tab_id(), tab));
}

pub(in crate::ui::options) fn page(cx: &mut Cx) {
    let p = cx.p;
    cx.group(tr!("fs-layouts-group"), |cx| {
        cx.note(tr!("fs-layouts-intro"));
        // (the search words of Folderstorm (C++): "dual monitor", "layout")
        cx.keywords("layout layouts profile dual monitor two monitors arrangement windows")
            .section_body()
            .row(tr!("fs-layouts-list"), tr!("fs-layouts-list-help"), &Nothing, |ui, s| {
                list(ui, &p, s)
            });
    });
    cx.group(tr!("fs-layouts-save-group"), |cx| {
        cx.keywords("layout save dual monitor").section_body().row(
            tr!("fs-layouts-save-new"),
            tr!("fs-layouts-save-new-help"),
            &Nothing,
            |ui, s| save_new(ui, &p, s),
        );
    });
    cx.group(tr!("fs-layouts-arrangement"), |cx| {
        cx.keywords("undo layout")
            .row(tr!("fs-layouts-previous"), tr!("fs-layouts-previous-help"), &Nothing, |ui, s| {
                let enabled = apply::has_previous(ui.ctx());
                let clicked = widgets::action_button_if(
                    ui,
                    &p,
                    ICON_BACK,
                    tr!("fs-layouts-previous-button"),
                    ActionKind::Flat,
                    vec2(0.0, BUTTON_H),
                    enabled,
                )
                .clicked();
                if !clicked || !apply::previous(ui.ctx(), &mut Live::of(s)) {
                    return false;
                }
                let mut tab = load(ui.ctx());
                tab.went_back = true;
                tab.selected = active_index(&s.folderstorm_workspaces).or(tab.selected);
                store(ui.ctx(), tab);
                true
            });
        cx.keywords("layout lock unlock window size borderless").row(
            tr!("fs-layouts-unlock"),
            tr!("fs-layouts-unlock-help"),
            &Nothing,
            |ui, s| {
                let held = s.folderstorm_workspaces.window_held;
                let clicked = widgets::action_button_if(
                    ui,
                    &p,
                    ICON_UNLOCK,
                    tr!("fs-layouts-unlock-button"),
                    ActionKind::Flat,
                    vec2(0.0, BUTTON_H),
                    held,
                )
                .clicked();
                clicked && apply::unlock(&mut s.folderstorm_workspaces)
            },
        );
        let policies = [
            (MonitorPolicy::Ask, tr!("fs-layouts-monitors-ask"), false),
            (MonitorPolicy::Apply, tr!("fs-layouts-monitors-apply"), false),
            (MonitorPolicy::Nothing, tr!("fs-layouts-monitors-nothing"), false),
        ];
        cx.keywords("layout dual monitor two monitors unplug screen change").wide().row(
            tr!("fs-layouts-monitors"),
            tr!("fs-layouts-monitors-help"),
            &Nothing,
            |ui, s| super::super::segmented(ui, &p, &mut s.folderstorm_workspaces.on_monitor_change, &policies),
        );
        cx.keywords("layout default bars viewport")
            .row(tr!("fs-layouts-reset"), tr!("fs-layouts-reset-help"), &Nothing, |ui, s| {
                let enabled = s.folderstorm_viewport != Default::default();
                let clicked = widgets::action_button_if(
                    ui,
                    &p,
                    ICON_RESET,
                    tr!("fs-layouts-reset-button"),
                    ActionKind::Flat,
                    vec2(0.0, BUTTON_H),
                    enabled,
                )
                .clicked();
                clicked && apply::reset_world(ui.ctx(), &mut Live::of(s))
            });
    });
    workspaces_group::groups(cx);
    cx.group(tr!("fs-layouts-quick"), |cx| {
        cx.note(tr!("fs-layouts-quick-intro"));
        for (place, label) in [(0, tr!("fs-layouts-toggle-first")), (1, tr!("fs-layouts-toggle-second"))] {
            cx.keywords("layout toggle switch switcher workspace toolbar button dual monitor two monitors shortcut keyboard favorite")
                .row(label, tr!("fs-layouts-toggle-help"), &Nothing, |ui, s| toggle_picker(ui, s, place));
        }
    });
}

/// One of the two layouts of the toolbar's toggle: a list of the usable
/// layouts, and "not chosen". True when the choice changed.
fn toggle_picker(ui: &mut egui::Ui, s: &mut Settings, place: usize) -> bool {
    let store = &mut s.folderstorm_workspaces;
    let mut choices: Vec<(Option<usize>, &str)> = vec![(None, tr!("fs-layouts-toggle-none"))];
    choices.extend(
        store
            .layouts
            .iter()
            .enumerate()
            .filter_map(|(index, record)| Some((Some(index), record.layout()?.name.as_str()))),
    );
    let chosen = store.toggle[place]
        .as_deref()
        .and_then(|name| store.find(name))
        .map(|(index, _)| index);
    let mut pick = chosen;
    let shown = choices.iter().find(|(value, _)| *value == chosen).map_or("", |(_, label)| *label);
    // (two lists in one group: each its own id)
    egui::ComboBox::from_id_salt(("folderstorm-layouts-toggle", place))
        .width(PICKER_W)
        .selected_text(shown)
        .truncate()
        .show_ui(ui, |ui| {
            for (value, label) in &choices {
                ui.selectable_value(&mut pick, *value, *label);
            }
        });
    if pick == chosen {
        return false;
    }
    let name = pick.and_then(|index| store.layout(index)).map(|layout| layout.name.clone());
    store.set_toggle(place, name.as_deref())
}

fn rename_id() -> egui::Id {
    egui::Id::unique("folderstorm_layouts_rename")
}

/// The rename field takes the keyboard with the whole name selected, so
/// that typing replaces it.
fn start_rename(ctx: &egui::Context, name: &str) {
    use egui::text::{CCursor, CCursorRange};
    let id = rename_id();
    let mut state = egui::TextEdit::load_state(ctx, id).unwrap_or_default();
    let all = CCursorRange::two(CCursor::new(0), CCursor::new(name.chars().count()));
    state.cursor.set_char_range(Some(all));
    state.store(ctx, id);
    ctx.memory_mut(|m| m.request_focus(id));
}

fn active_index(store: &Store) -> Option<usize> {
    (0..store.layouts.len()).find(|index| store.is_active(*index))
}

fn muted(ui: &mut egui::Ui, p: &Palette, text: impl Into<String>) {
    ui.add(egui::Label::new(egui::RichText::new(text).size(11.5).color(p.muted)).wrap());
}

fn warning(ui: &mut egui::Ui, p: &Palette, text: impl Into<String>) {
    ui.add(egui::Label::new(egui::RichText::new(text).size(11.5).color(p.danger)).wrap());
}

/// One line of the list: the mark of the layout in use, the name, a note
/// at the right ("In use", "Modified", why it cannot be used).
#[allow(clippy::too_many_arguments)]
fn entry(
    ui: &mut egui::Ui,
    p: &Palette,
    name: &str,
    selected: bool,
    active: bool,
    usable: bool,
    locked: bool,
    favorite: bool,
    note: &str,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
    let fill = if selected {
        p.violet.gamma_multiply(0.35)
    } else if response.hovered() {
        p.raised
    } else {
        p.field
    };
    ui.painter().rect_filled(rect, widgets::inner_radius(ui.visuals()), fill);
    if active && let Some(icon) = crate::ui::icons::global(ICON_CHECK) {
        ui.painter().image(
            icon.id(),
            egui::Rect::from_center_size(pos2(rect.left() + 16.0, rect.center().y), Vec2::splat(14.0)),
            egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            p.teal,
        );
    }
    let note_ink = if usable { p.muted } else { p.danger };
    let note_galley = ui
        .painter()
        .layout_no_wrap(note.to_owned(), egui::FontId::proportional(11.5), note_ink);
    let mut note_w = if note.is_empty() { 0.0 } else { note_galley.size().x + 12.0 };
    // a layout that locks the viewer window: a lock before its note
    if locked && let Some(icon) = crate::ui::icons::global(ICON_LOCK) {
        note_w += 20.0;
        ui.painter().image(
            icon.id(),
            egui::Rect::from_center_size(pos2(rect.right() - note_w, rect.center().y), Vec2::splat(13.0)),
            egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            p.muted,
        );
        note_w += 8.0;
    }
    // a favorite: its star, before the lock and the note
    if favorite && let Some(icon) = crate::ui::icons::global(ICON_STAR) {
        note_w += 20.0;
        ui.painter().image(
            icon.id(),
            egui::Rect::from_center_size(pos2(rect.right() - note_w, rect.center().y), Vec2::splat(13.0)),
            egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            p.warn,
        );
        note_w += 8.0;
    }
    let name_left = rect.left() + 30.0;
    let ink = if usable { p.ink } else { p.muted };
    widgets::paint_elided(
        ui,
        name,
        pos2(name_left, rect.center().y),
        (rect.right() - 10.0 - note_w - name_left).max(1.0),
        egui::FontId::proportional(13.0),
        ink,
    );
    if !note.is_empty() {
        let at = pos2(
            rect.right() - 10.0 - note_galley.size().x,
            rect.center().y - note_galley.size().y / 2.0,
        );
        ui.painter().galley(at, note_galley, note_ink);
    }
    let lock = if locked { tr!("fs-layouts-locked") } else { "" };
    let star = if favorite { tr!("fs-layouts-favorite") } else { "" };
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, format!("{name} {star} {lock} {note}")));
    response
}

/// The note at the right of a layout's line.
fn note(store: &Store, world: &crate::folderstorm::viewport::ViewportSettings, index: usize, record: &Record) -> &'static str {
    match record {
        Record::Kept(kept) => match kept.why {
            Unusable::Newer => tr!("fs-layouts-newer"),
            Unusable::Invalid => tr!("fs-layouts-invalid"),
        },
        Record::Layout(_) if !store.is_active(index) => "",
        Record::Layout(_) if apply::modified(store, world) => tr!("fs-layouts-modified"),
        Record::Layout(_) => tr!("fs-layouts-active"),
    }
}

/// The list and the selected layout's actions. True when the settings
/// changed.
fn list(ui: &mut egui::Ui, p: &Palette, s: &mut Settings) -> bool {
    let ctx = ui.ctx().clone();
    let mut tab = load(&ctx);
    let before = tab.clone();
    let count = s.folderstorm_workspaces.layouts.len();
    if tab.selected.is_some_and(|index| index >= count) {
        tab.selected = None;
    }
    if tab.selected.is_none() {
        tab.selected = active_index(&s.folderstorm_workspaces);
        tab.rename = None;
        tab.ask = None;
    }
    let mut changed = false;
    ui.vertical(|ui| {
        ui.set_width(ui.available_width().min(LIST_W));
        ui.spacing_mut().item_spacing.y = 4.0;
        if count == 0 {
            muted(ui, p, tr!("fs-layouts-empty"));
        }
        for index in 0..count {
            let store = &s.folderstorm_workspaces;
            let record = &store.layouts[index];
            let name = record.name().unwrap_or(tr!("fs-layouts-unnamed"));
            let selected = tab.selected == Some(index);
            let line = entry(
                ui,
                p,
                name,
                selected,
                store.is_active(index),
                record.layout().is_some(),
                record
                    .layout()
                    .and_then(|layout| layout.window.as_ref())
                    .is_some_and(WindowFrame::locked),
                record.layout().is_some_and(|layout| store.is_favorite(&layout.name)),
                note(store, &s.folderstorm_viewport, index, record),
            );
            if line.clicked() && !selected {
                tab.selected = Some(index);
                tab.rename = None;
                tab.ask = None;
                tab.refused = None;
            }
            // a double click applies, as Enter will in the switcher
            if line.double_clicked() && record.layout().is_some() {
                changed |= do_apply(&ctx, s, &mut tab, index);
            }
        }
        if let Some(index) = tab.selected {
            ui.add_space(4.0);
            changed |= selected_actions(ui, p, s, &mut tab, index);
        }
        let waiting = s.folderstorm_workspaces.pending.len();
        if tab.went_back {
            muted(ui, p, tr!("fs-layouts-previous-done"));
        } else if let Some(report) = apply::report(&ctx).filter(|report| report.workspace.is_none()) {
            // (a workspace's is said under the workspaces)
            muted(ui, p, report_text(&report, waiting));
        }
    });
    if tab_differs(&tab, &before) {
        store(&ctx, tab);
    }
    changed
}

fn tab_differs(a: &Tab, b: &Tab) -> bool {
    a.selected != b.selected
        || a.new_name != b.new_name
        || a.parts != b.parts
        || a.rename != b.rename
        || a.ask != b.ask
        || a.refused != b.refused
        || a.went_back != b.went_back
}

fn do_apply(ctx: &egui::Context, s: &mut Settings, tab: &mut Tab, index: usize) -> bool {
    tab.ask = None;
    tab.rename = None;
    tab.went_back = false;
    match apply::apply(ctx, &mut Live::of(s), index) {
        Ok(_) => {
            tab.refused = None;
            true
        }
        Err(error) => {
            tab.refused = Some(Refused::Selected(error));
            false
        }
    }
}

/// Under the list: what the selected entry holds and what can be done with
/// it. True when the settings changed.
fn selected_actions(ui: &mut egui::Ui, p: &Palette, s: &mut Settings, tab: &mut Tab, index: usize) -> bool {
    let ctx = ui.ctx().clone();
    let Some(record) = s.folderstorm_workspaces.layouts.get(index).cloned() else {
        return false;
    };
    let name = record.name().unwrap_or(tr!("fs-layouts-unnamed")).to_owned();
    let usable = record.layout().is_some();
    let count = s.folderstorm_workspaces.layouts.len();
    let mut changed = false;
    match &record {
        Record::Layout(layout) => muted(ui, p, words::holds(layout)),
        Record::Kept(kept) => muted(
            ui,
            p,
            match kept.why {
                Unusable::Newer => tr!("fs-layouts-newer-help"),
                Unusable::Invalid => tr!("fs-layouts-invalid-help"),
            },
        ),
    }
    // a name being typed, a question being asked, or the buttons
    if let Some(typed) = tab.rename.as_mut() {
        let mut done = None;
        ui.horizontal(|ui| {
            let field = ui.add(
                egui::TextEdit::singleline(typed)
                    .id(rename_id())
                    .desired_width(240.0)
                    .char_limit(MAX_NAME * 2),
            );
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if widgets::action_button(
                ui,
                p,
                ICON_CHECK,
                tr!("fs-layouts-rename"),
                ActionKind::Primary,
                vec2(0.0, BUTTON_H),
            )
            .clicked()
                || enter
            {
                done = Some(true);
            }
            if widgets::action_button(ui, p, ICON_CANCEL, tr!("common-cancel"), ActionKind::Flat, vec2(0.0, BUTTON_H)).clicked() {
                done = Some(false);
            }
        });
        match done {
            Some(true) => match s.folderstorm_workspaces.rename(index, typed) {
                Ok(()) => {
                    tab.rename = None;
                    tab.refused = None;
                    changed = true;
                }
                Err(error) => tab.refused = Some(Refused::Rename(error)),
            },
            Some(false) => {
                tab.rename = None;
                tab.refused = None;
            }
            None => {}
        }
    } else if let Some(ask) = tab.ask {
        let (question, icon, label, kind) = match ask {
            Ask::Update => (
                tr!("fs-layouts-ask-update", name = name.as_str()),
                ICON_SAVE,
                tr!("fs-layouts-update"),
                ActionKind::Primary,
            ),
            Ask::Delete => (
                tr!("fs-layouts-ask-delete", name = name.as_str()),
                ICON_DELETE,
                tr!("common-delete"),
                ActionKind::Danger,
            ),
        };
        ui.add(egui::Label::new(egui::RichText::new(question).size(12.5).color(p.ink)).wrap());
        let mut answer = None;
        ui.horizontal(|ui| {
            if widgets::action_button(ui, p, icon, label, kind, vec2(0.0, BUTTON_H)).clicked() {
                answer = Some(true);
            }
            if widgets::action_button(ui, p, ICON_CANCEL, tr!("common-cancel"), ActionKind::Flat, vec2(0.0, BUTTON_H)).clicked() {
                answer = Some(false);
            }
        });
        if let Some(yes) = answer {
            tab.ask = None;
            if yes {
                tab.went_back = false;
                match ask {
                    Ask::Update => match apply::update(&ctx, &mut Live::of(s), index) {
                        Ok(()) => {
                            tab.refused = None;
                            changed = true;
                        }
                        Err(error) => tab.refused = Some(Refused::Selected(error)),
                    },
                    Ask::Delete => {
                        changed |= s.folderstorm_workspaces.remove(index);
                        tab.selected = None;
                        tab.refused = None;
                    }
                }
            }
        }
    } else {
        ui.horizontal_wrapped(|ui| {
            let size = vec2(0.0, BUTTON_H);
            if widgets::action_button_if(ui, p, ICON_CHECK, tr!("fs-layouts-apply"), ActionKind::Primary, size, usable).clicked() {
                changed |= do_apply(&ctx, s, tab, index);
            }
            if widgets::action_button_if(ui, p, ICON_SAVE, tr!("fs-layouts-update"), ActionKind::Flat, size, usable).clicked() {
                tab.ask = Some(Ask::Update);
            }
            if widgets::action_button_if(ui, p, ICON_RENAME, tr!("fs-layouts-rename"), ActionKind::Flat, size, usable).clicked() {
                start_rename(&ctx, &name);
                tab.rename = Some(name.clone());
            }
            if widgets::action_button(ui, p, ICON_DELETE, tr!("common-delete"), ActionKind::FlatDanger, size).clicked() {
                tab.ask = Some(Ask::Delete);
            }
            if widgets::icon_button(ui, p, ICON_UP, tr!("fs-layouts-move-up"), index > 0)
                && let Some(to) = s.folderstorm_workspaces.shift(index, true)
            {
                tab.selected = Some(to);
                changed = true;
            }
            if widgets::icon_button(ui, p, ICON_DOWN, tr!("fs-layouts-move-down"), index + 1 < count)
                && let Some(to) = s.folderstorm_workspaces.shift(index, false)
            {
                tab.selected = Some(to);
                changed = true;
            }
            // a favorite: first in the switcher, in World › Layouts, and in
            // the keyboard's round
            let favorite = usable && s.folderstorm_workspaces.is_favorite(&name);
            let star_tip = match favorite {
                true => tr!("fs-switcher-unstar"),
                false => tr!("fs-switcher-star"),
            };
            if usable && widgets::icon_toggle(ui, p, ICON_STAR, star_tip, favorite) {
                changed |= s.folderstorm_workspaces.set_favorite(&name, !favorite);
            }
        });
    }
    if let Some(Refused::Rename(error) | Refused::Selected(error)) = tab.refused {
        warning(ui, p, refusal(error));
    }
    if tab.rename.is_none()
        && tab.ask.is_none()
        && let Some(layout) = record.layout()
        && let Some(frame) = &layout.window
        && let Some(edited) = window_editor(ui, p, layout, frame, apply::viewer_window(&ctx))
    {
        match apply::set_window(&mut s.folderstorm_workspaces, index, edited) {
            Ok(()) => {
                tab.refused = None;
                changed = true;
            }
            Err(error) => tab.refused = Some(Refused::Selected(error)),
        }
    }
    changed
}

/// The three modes of a viewer window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Normal,
    Fullscreen,
    Borderless,
}

fn mode_of(frame: &WindowFrame) -> Mode {
    match frame {
        WindowFrame::Normal { .. } => Mode::Normal,
        WindowFrame::Fullscreen { .. } => Mode::Fullscreen,
        WindowFrame::Borderless { .. } => Mode::Borderless,
    }
}

fn monitor_rect(m: &Monitor) -> [i32; 4] {
    [
        m.x,
        m.y,
        i32::try_from(m.width).unwrap_or(i32::MAX),
        i32::try_from(m.height).unwrap_or(i32::MAX),
    ]
}

/// Where `frame` puts the window on the monitors it was saved on, and the
/// places of the monitors it names.
fn frame_on(frame: &WindowFrame, saved: &[Monitor]) -> (Option<[i32; 4]>, Vec<usize>) {
    match frame {
        WindowFrame::Normal { x, y, width, height, .. } => (
            x.zip(*y).map(|(x, y)| {
                [
                    x,
                    y,
                    i32::try_from(*width).unwrap_or(i32::MAX),
                    i32::try_from(*height).unwrap_or(i32::MAX),
                ]
            }),
            Vec::new(),
        ),
        WindowFrame::Fullscreen { monitor } => {
            let n = usize::from(*monitor);
            (saved.get(n).map(monitor_rect), vec![n])
        }
        WindowFrame::Borderless { monitors: chosen, .. } => {
            let places: Vec<usize> = chosen.iter().map(|n| usize::from(*n)).collect();
            (monitors::cover(places.iter().filter_map(|n| saved.get(*n))), places)
        }
    }
}

/// `frame` in another mode, on the monitors `saved`: the monitor a normal
/// window is on for full screen and borderless; the rectangle of the
/// monitors named, a little inside it, for a normal window. `now` is the
/// viewer window as it is, when it can be read as a normal one.
fn in_mode(frame: &WindowFrame, mode: Mode, saved: &[Monitor], now: Option<WindowFrame>) -> WindowFrame {
    let (rect, places) = frame_on(frame, saved);
    let centre = rect.map(|[x, y, w, h]| (i64::from(x) + i64::from(w) / 2, i64::from(y) + i64::from(h) / 2));
    let on = places
        .first()
        .copied()
        .or_else(|| centre.and_then(|(x, y)| monitors::monitor_at(x, y, saved)))
        .unwrap_or(0);
    let on = u8::try_from(on).unwrap_or(0);
    let locked = frame.locked();
    match mode {
        Mode::Fullscreen => WindowFrame::Fullscreen { monitor: on },
        Mode::Borderless => WindowFrame::Borderless {
            monitors: if places.is_empty() {
                vec![on]
            } else {
                places.iter().filter_map(|n| u8::try_from(*n).ok()).collect()
            },
            locked,
        },
        Mode::Normal => match (now, rect) {
            (Some(now @ WindowFrame::Normal { .. }), _) => now.with_lock(locked),
            (_, Some([x, y, w, h])) => WindowFrame::Normal {
                x: Some(x.saturating_add(w / 10)),
                y: Some(y.saturating_add(h / 10)),
                width: (w - w / 5).max(1) as u32,
                height: (h - h / 5).max(1) as u32,
                maximized: false,
                locked,
            },
            _ => WindowFrame::Normal {
                x: None,
                y: None,
                width: 1280,
                height: 720,
                maximized: false,
                locked,
            },
        },
    }
}

/// `frame` after a click on the monitor at `place`: full screen goes
/// there; borderless takes it in or lets it go (never its last one).
fn with_monitor(frame: &WindowFrame, place: usize) -> Option<WindowFrame> {
    let place = u8::try_from(place).ok()?;
    match frame {
        WindowFrame::Normal { .. } => None,
        WindowFrame::Fullscreen { .. } => Some(WindowFrame::Fullscreen { monitor: place }),
        WindowFrame::Borderless { monitors: chosen, locked } => {
            let mut chosen = chosen.clone();
            match chosen.iter().position(|n| *n == place) {
                Some(_) if chosen.len() == 1 => return None,
                Some(at) => {
                    chosen.remove(at);
                }
                None => chosen.push(place),
            }
            chosen.sort_unstable();
            Some(WindowFrame::Borderless {
                monitors: chosen,
                locked: *locked,
            })
        }
    }
}

/// The selected layout's viewer window: its mode, its monitors on a small
/// picture of the layout's own, its lock box. The frame as edited, when it
/// was.
fn window_editor(ui: &mut egui::Ui, p: &Palette, layout: &Layout, frame: &WindowFrame, now: Option<WindowFrame>) -> Option<WindowFrame> {
    let saved = &layout.monitors;
    let mut edited = None;
    ui.add_space(4.0);
    ui.label(egui::RichText::new(tr!("fs-layouts-part-window")).size(12.5).color(p.ink));
    let mut mode = mode_of(frame);
    // full screen and borderless name monitors of the layout's own list
    let modes = [
        (Mode::Normal, tr!("fs-layouts-mode-normal"), false),
        (Mode::Fullscreen, tr!("fs-layouts-mode-fullscreen"), false),
        (Mode::Borderless, tr!("fs-layouts-mode-borderless"), false),
    ];
    ui.add_enabled_ui(!saved.is_empty(), |ui| {
        if super::super::segmented(ui, p, &mut mode, &modes) {
            edited = Some(in_mode(frame, mode, saved, now));
        }
    });
    if saved.is_empty() {
        muted(ui, p, tr!("fs-layouts-mode-no-monitors"));
    } else {
        let rects: Vec<[i32; 4]> = saved.iter().map(monitor_rect).collect();
        let (window, chosen) = frame_on(frame, saved);
        let pick = mode != Mode::Normal;
        if let Some(place) = crate::folderstorm::diagram::widget::monitors(ui, p, &rects, window, &chosen, pick) {
            edited = with_monitor(frame, place).or(edited);
        }
        match mode {
            Mode::Normal => {}
            Mode::Fullscreen => muted(ui, p, tr!("fs-layouts-mode-pick-fullscreen")),
            Mode::Borderless => muted(ui, p, tr!("fs-layouts-mode-pick-borderless")),
        }
    }
    if mode != Mode::Fullscreen {
        let mut locked = frame.locked();
        if widgets::checkbox(ui, p, &mut locked, tr!("fs-layouts-lock")).changed() {
            edited = Some(frame.clone().with_lock(locked));
        }
    }
    edited
}

/// *Save as a new layout*: a name, the parts, Save. True when the settings
/// changed.
fn save_new(ui: &mut egui::Ui, p: &Palette, s: &mut Settings) -> bool {
    let ctx = ui.ctx().clone();
    let mut tab = load(&ctx);
    let before = tab.clone();
    let mut changed = false;
    ui.vertical(|ui| {
        ui.set_width(ui.available_width().min(LIST_W));
        muted(ui, p, tr!("fs-layouts-save-new-help"));
        let mut save = false;
        ui.horizontal(|ui| {
            let field = ui.add(
                egui::TextEdit::singleline(&mut tab.new_name)
                    .desired_width(240.0)
                    .char_limit(MAX_NAME * 2)
                    .hint_text(tr!("fs-layouts-name-hint")),
            );
            save |= field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            save |= widgets::action_button(ui, p, ICON_SAVE, tr!("fs-layouts-save"), ActionKind::Primary, vec2(0.0, BUTTON_H)).clicked();
        });
        // (no viewer window to read under a forced size or a capture)
        let readable = apply::has_viewer_window(&ctx);
        ui.horizontal_wrapped(|ui| {
            ui.add_enabled_ui(readable, |ui| {
                let mut window = tab.parts.window && readable;
                if widgets::checkbox(ui, p, &mut window, tr!("fs-layouts-part-window")).changed() {
                    tab.parts.window = window;
                }
            });
            widgets::checkbox(ui, p, &mut tab.parts.world, tr!("fs-layouts-part-world"));
            widgets::checkbox(ui, p, &mut tab.parts.windows, tr!("fs-layouts-part-windows"));
        });
        let with_window = tab.parts.window && readable;
        ui.add_enabled_ui(with_window, |ui| {
            let mut lock = tab.parts.lock && with_window;
            if widgets::checkbox(ui, p, &mut lock, tr!("fs-layouts-lock")).changed() {
                tab.parts.lock = lock;
            }
        });
        if !with_window && !tab.parts.world && !tab.parts.windows {
            warning(ui, p, tr!("fs-layouts-error-parts"));
        } else if save {
            match apply::save_new(&ctx, &mut Live::of(s), &tab.new_name, tab.parts) {
                Ok(index) => {
                    tab.new_name.clear();
                    tab.selected = Some(index);
                    tab.rename = None;
                    tab.ask = None;
                    tab.refused = None;
                    tab.went_back = false;
                    changed = true;
                }
                Err(error) => tab.refused = Some(Refused::Save(error)),
            }
        }
        if let Some(Refused::Save(error)) = tab.refused {
            warning(ui, p, refusal(error));
        }
    });
    if tab_differs(&tab, &before) {
        store(&ctx, tab);
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::folderstorm::workspaces::apply::Report;
    use crate::folderstorm::workspaces::window::{Command, Pretend};
    use crate::ui::options::{OptionsUi, Page, show};
    use crate::ui::widgets::Floater;
    use crate::ui::widgets::elide_tests::{Frames, Text};
    use egui::Rect;

    mod workspaces;

    /// Preferences on the Layouts tab, over a viewer with two windows open.
    struct Window {
        frames: Frames,
        p: Palette,
        icons: crate::ui::icons::Icons,
        s: Settings,
        st: OptionsUi,
        changed: bool,
        /// A pretend viewer window over two pretend monitors, when the
        /// test has one (`Window::with_viewer`).
        viewer: Option<Pretend>,
    }

    const MONITORS: [[i32; 4]; 2] = [[0, 0, 1920, 1080], [1920, 0, 1920, 1080]];

    impl Window {
        /// The same, with a viewer window the layouts can read and drive,
        /// on the left one of two monitors.
        fn with_viewer() -> Self {
            let mut window = Window::new();
            window.viewer = Some(Pretend::new(100, 60, 1280, 900));
            window.settle();
            window
        }

        fn seen(&self) -> crate::folderstorm::workspaces::window::Seen {
            self.viewer.as_ref().expect("a pretend window").seen
        }

        fn new() -> Self {
            let frames = Frames::new();
            let theme = crate::theme::Theme::default();
            theme.apply(&frames.ctx, 1.0);
            frames.ctx.all_styles_mut(|s| s.animation_time = 0.0);
            let icons = crate::ui::icons::Icons::load(&frames.ctx, None);
            let mut st = OptionsUi::default();
            st.open(Page::Layout, 3);
            let mut window = Window {
                frames,
                p: theme.palette(),
                icons,
                s: Settings::default(),
                st,
                changed: false,
                viewer: None,
            };
            window.run(Vec::new());
            // the two windows out of the way of Preferences, at known places
            widgets::place_floater(&window.frames.ctx, "inventory", pos2(20.0, 600.0), vec2(300.0, 280.0));
            widgets::place_floater(&window.frames.ctx, "people", pos2(340.0, 600.0), vec2(260.0, 280.0));
            window.settle();
            window
        }

        /// One frame: the app's call before the pass, two of the viewer's
        /// windows, then Preferences.
        fn run(&mut self, events: Vec<egui::Event>) -> Vec<Text> {
            let ctx = self.frames.ctx.clone();
            if let Some(viewer) = &self.viewer {
                crate::folderstorm::diagram::set_desktop(
                    &ctx,
                    crate::folderstorm::diagram::Desktop {
                        window: viewer.seen.inner,
                        position: viewer.seen.outer,
                        monitors: MONITORS
                            .iter()
                            .map(|rect| crate::folderstorm::diagram::Monitor { rect: *rect, scale: 1.0 })
                            .collect(),
                    },
                );
            }
            let outcome = apply::frame(
                &ctx,
                &mut Live::of(&mut self.s),
                self.viewer.as_ref().map(|viewer| viewer.seen),
                &mut apply::NoShell,
            );
            self.changed |= outcome.save;
            if let Some(viewer) = &mut self.viewer {
                viewer.send(&outcome.commands);
                viewer.frame();
            }
            let (p, icons, s, st, changed) = (&self.p, &self.icons, &mut self.s, &mut self.st, &mut self.changed);
            self.frames.run(events, |ui| {
                let ctx = ui.ctx();
                let screen = ctx.content_rect();
                crate::ui::layout::set_work_area(ctx, screen);
                crate::ui::layout::set_move_area(ctx, screen);
                for id in ["inventory", "people"] {
                    Floater::new(id, id, pos2(20.0, 600.0), vec2(300.0, 280.0)).show(ctx, p, &mut true, |ui| ui.label("…"));
                }
                *changed |= show(ctx, p, icons, s, st, &mut true).changed;
            })
        }

        fn settle(&mut self) -> Vec<Text> {
            let mut texts = Vec::new();
            for _ in 0..50 {
                texts = self.run(Vec::new());
            }
            texts
        }

        fn rect_of(&mut self, text: &str) -> Option<Rect> {
            let texts = self.run(Vec::new());
            texts.iter().find(|t| t.source == text).map(|t| t.rect)
        }

        fn shows(&mut self, text: &str) -> bool {
            self.rect_of(text).is_some()
        }

        fn click_at(&mut self, at: egui::Pos2) {
            let button = |pressed| egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            };
            for events in [
                vec![egui::Event::PointerMoved(at)],
                vec![button(true)],
                vec![button(false)],
                Vec::new(),
                Vec::new(),
            ] {
                self.run(events);
            }
        }

        /// Clicks the text `label` (a button's, a line's of the list).
        fn click(&mut self, label: &str) {
            let rect = self.rect_of(label).unwrap_or_else(|| panic!("“{label}” is shown"));
            self.click_at(rect.center());
        }

        fn type_text(&mut self, text: &str) {
            self.run(vec![egui::Event::Text(text.to_owned())]);
            self.run(Vec::new());
        }

        fn save_as(&mut self, name: &str) {
            self.click(tr!("fs-layouts-name-hint"));
            self.type_text(name);
            self.click(tr!("fs-layouts-save"));
            self.settle();
        }

        fn store(&self) -> &Store {
            &self.s.folderstorm_workspaces
        }
    }

    fn wide() -> crate::folderstorm::viewport::ViewportSettings {
        crate::folderstorm::viewport::ViewportSettings {
            enabled: true,
            left: 400,
            bars_around: true,
            ..Default::default()
        }
    }

    #[test]
    fn the_tab_saves_the_arrangement_under_a_name_and_marks_it_in_use() {
        let mut w = Window::new();
        assert!(w.shows(tr!("fs-layouts-empty")));
        assert!(!w.shows(tr!("fs-layouts-apply")), "nothing selected, no actions");
        w.s.folderstorm_viewport = wide();
        w.changed = false;
        w.save_as("Wide");
        assert!(w.changed, "the settings are saved");
        let layout = w.store().layout(0).expect("saved");
        assert_eq!(layout.name, "Wide");
        assert_eq!(layout.world.as_ref(), Some(&wide()));
        let windows = layout.windows.as_ref().expect("the windows part");
        assert_eq!(
            windows.windows.keys().map(String::as_str).collect::<Vec<_>>(),
            ["inventory", "people"]
        );
        assert_eq!((windows.windows["inventory"].x, windows.windows["inventory"].y), (20.0, 600.0));
        // listed, selected, in use; the field is ready for another name
        assert!(w.shows("Wide") && w.shows(tr!("fs-layouts-active")));
        assert!(!w.shows(tr!("fs-layouts-empty")));
        assert!(w.shows(tr!("fs-layouts-name-hint")));
        assert!(w.shows(tr!("fs-layouts-apply")));
        let holds = format!("{} · {}", tr!("fs-layouts-part-world"), tr!("fs-layouts-holds-windows", count = 2));
        assert!(w.shows(&holds), "what it holds");
    }

    #[test]
    fn a_name_that_cannot_be_used_is_refused_with_the_reason() {
        let mut w = Window::new();
        w.click(tr!("fs-layouts-save"));
        assert!(w.shows(tr!("fs-layouts-error-empty")));
        assert!(w.store().layouts.is_empty());
        w.save_as("Wide");
        assert!(!w.shows(tr!("fs-layouts-error-empty")));
        w.save_as("  wide ");
        assert!(w.shows(tr!("fs-layouts-error-taken")));
        assert_eq!(w.store().layouts.len(), 1);
        // without a part there is nothing to save
        w.click(tr!("fs-layouts-part-world"));
        w.click(tr!("fs-layouts-part-windows"));
        assert!(w.shows(tr!("fs-layouts-error-parts")));
        w.click(tr!("fs-layouts-save"));
        assert_eq!(w.store().layouts.len(), 1);
        // one part is enough: the world alone
        w.click(tr!("fs-layouts-part-world"));
        w.type_text("");
        w.click(tr!("fs-layouts-save"));
        w.settle();
        assert_eq!(w.store().layouts.len(), 1, "still named like the first");
    }

    #[test]
    fn apply_puts_the_layout_back_and_says_what_it_did() {
        let mut w = Window::new();
        w.s.folderstorm_viewport = wide();
        w.save_as("Wide");
        w.s.folderstorm_viewport = Default::default();
        widgets::move_floater(&w.frames.ctx, "people", pos2(700.0, 620.0));
        w.settle();
        w.save_as("Single");
        assert!(w.store().is_active(1));
        assert!(!w.shows(tr!("fs-layouts-previous-done")));
        // select "Wide", apply
        w.click("Wide");
        w.changed = false;
        w.click(tr!("fs-layouts-apply"));
        w.settle();
        assert!(w.changed);
        assert_eq!(w.s.folderstorm_viewport, wide());
        assert!(w.store().is_active(0));
        let people = w.frames.ctx.memory(|m| m.area_rect(egui::Id::unique("people"))).expect("shown");
        assert_eq!(people.min, pos2(340.0, 600.0));
        assert!(w.shows(&tr!("fs-layouts-applied-windows", name = "Wide", count = 2)));
        // moving a window marks it
        widgets::move_floater(&w.frames.ctx, "people", pos2(700.0, 620.0));
        w.settle();
        assert!(w.shows(tr!("fs-layouts-modified")));
        // Previous arrangement: "Single" again, and its line selected
        w.click(tr!("fs-layouts-previous-button"));
        w.settle();
        assert_eq!(w.s.folderstorm_viewport, Default::default());
        assert!(w.store().is_active(1));
        assert!(w.shows(tr!("fs-layouts-previous-done")));
        // a double click on a line applies it
        let line = w.rect_of("Wide").expect("listed").center();
        w.click_at(line);
        w.click_at(line);
        w.settle();
        assert!(w.store().is_active(0), "double-clicked");
    }

    #[test]
    fn update_and_delete_ask_first() {
        let mut w = Window::new();
        w.save_as("Wide");
        let saved = w.store().clone();
        w.s.folderstorm_viewport = wide();
        w.settle();
        assert!(w.shows(tr!("fs-layouts-modified")), "the world is no longer the layout's");
        // Update: asked, cancelled, nothing changes
        w.click(tr!("fs-layouts-update"));
        let question = tr!("fs-layouts-ask-update", name = "Wide");
        assert!(w.shows(&question));
        assert_eq!(w.store(), &saved);
        w.click(tr!("common-cancel"));
        assert!(!w.shows(&question));
        assert_eq!(w.store(), &saved);
        // Update: asked, confirmed
        w.click(tr!("fs-layouts-update"));
        w.click(tr!("fs-layouts-update"));
        w.settle();
        assert_eq!(w.store().layout(0).and_then(|l| l.world.clone()), Some(wide()));
        assert!(w.shows(tr!("fs-layouts-active")) && !w.shows(tr!("fs-layouts-modified")));
        // Delete: asked, cancelled, then confirmed
        w.click(tr!("common-delete"));
        let question = tr!("fs-layouts-ask-delete", name = "Wide");
        assert!(w.shows(&question));
        w.click(tr!("common-cancel"));
        assert_eq!(w.store().layouts.len(), 1);
        w.click(tr!("common-delete"));
        w.click(tr!("common-delete"));
        w.settle();
        assert!(w.store().layouts.is_empty());
        assert_eq!(w.store().active_layout, None);
        assert!(w.shows(tr!("fs-layouts-empty")));
        // deleting a layout changes nothing of the arrangement
        assert_eq!(w.s.folderstorm_viewport, wide());
    }

    #[test]
    fn a_layout_is_renamed_and_moved_in_the_list() {
        let mut w = Window::new();
        w.save_as("Wide");
        w.save_as("Single");
        w.click("Wide");
        w.click(tr!("fs-layouts-rename"));
        // the field has the keyboard, its name selected: typing replaces it
        w.type_text("Wide screen");
        w.click(tr!("fs-layouts-rename"));
        w.settle();
        assert_eq!(w.store().layout(0).map(|l| l.name.as_str()), Some("Wide screen"));
        // a name already taken is refused and the field stays
        w.click(tr!("fs-layouts-rename"));
        w.type_text("single");
        w.click(tr!("fs-layouts-rename"));
        assert!(w.shows(tr!("fs-layouts-error-taken")));
        assert_eq!(w.store().layout(0).map(|l| l.name.as_str()), Some("Wide screen"));
        w.click(tr!("common-cancel"));
        assert!(!w.shows(tr!("fs-layouts-error-taken")));
        // "Single" is in use; moved up, it still is
        assert!(w.store().is_active(1));
        w.click("Single");
        let up = w.rect_of(tr!("common-delete")).expect("the actions").right_center() + vec2(20.0, 0.0);
        w.click_at(up);
        w.settle();
        assert_eq!(w.store().layout(0).map(|l| l.name.as_str()), Some("Single"));
        assert!(w.store().is_active(0));
    }

    #[test]
    fn an_entry_that_cannot_be_used_is_listed_and_can_only_be_deleted() {
        let mut w = Window::new();
        w.s.folderstorm_workspaces = serde_json::from_value(serde_json::json!({
            "layouts": [
                { "schema": 7, "name": "From later", "world": "another shape" },
                Layout::new("Wide", Some(wide()), None, Vec::new()),
            ],
            "active_layout": "Wide",
        }))
        .expect("read");
        let saved = w.store().clone();
        w.settle();
        assert!(w.shows("From later") && w.shows(tr!("fs-layouts-newer")));
        w.click("From later");
        assert!(w.shows(tr!("fs-layouts-newer-help")));
        // Apply, Update and Rename are greyed: clicking them does nothing
        for label in [tr!("fs-layouts-apply"), tr!("fs-layouts-update"), tr!("fs-layouts-rename")] {
            w.click(label);
        }
        w.settle();
        assert_eq!(w.store(), &saved);
        assert_eq!(w.s.folderstorm_viewport, Default::default());
        assert!(!w.shows(&tr!("fs-layouts-ask-update", name = "From later")));
        // it is written back as it was read
        assert_eq!(
            serde_json::to_value(w.store()).expect("saved")["layouts"][0],
            serde_json::json!({ "schema": 7, "name": "From later", "world": "another shape" })
        );
        // and can be deleted
        w.click(tr!("common-delete"));
        w.click(tr!("common-delete"));
        w.settle();
        assert_eq!(w.store().layouts.len(), 1);
        assert!(w.store().is_active(0));
    }

    #[test]
    fn reset_bars_and_world_and_previous_are_greyed_until_they_have_something_to_do() {
        let mut w = Window::new();
        // nothing to reset, nothing to go back to: clicks do nothing
        w.click(tr!("fs-layouts-reset-button"));
        w.click(tr!("fs-layouts-previous-button"));
        assert!(!w.changed);
        w.s.folderstorm_viewport = wide();
        w.click(tr!("fs-layouts-reset-button"));
        w.settle();
        assert_eq!(w.s.folderstorm_viewport, Default::default());
        assert!(w.changed);
        // and Previous brings the viewport back
        w.click(tr!("fs-layouts-previous-button"));
        w.settle();
        assert_eq!(w.s.folderstorm_viewport, wide());
    }

    #[test]
    fn the_search_finds_the_tab_by_dual_monitor_and_layout() {
        for query in ["dual monitor", "layout"] {
            let mut w = Window::new();
            w.st.query = query.to_owned();
            w.settle();
            assert!(w.shows(tr!("fs-layouts-list")), "{query}");
        }
    }

    #[test]
    fn resetting_the_page_keeps_the_layouts() {
        let mut w = Window::new();
        w.save_as("Wide");
        let saved = w.store().clone();
        let d = Settings::default();
        let mut r = crate::ui::options::OptionsResult::default();
        let (p, s, st) = (w.p, &mut w.s, &mut w.st);
        w.frames.run(Vec::new(), |ui| {
            let mut cx = Cx::new(ui, p, s, &d, st, &mut r, super::super::super::cx::Mode::Reset, "");
            cx.enter(Page::Layout, 3);
            page(&mut cx);
        });
        assert_eq!(w.store(), &saved);
    }

    fn two_monitors() -> Vec<Monitor> {
        MONITORS
            .iter()
            .map(|[x, y, width, height]| Monitor {
                x: *x,
                y: *y,
                width: *width as u32,
                height: *height as u32,
                scale: 1.0,
            })
            .collect()
    }

    #[test]
    fn a_layout_is_saved_with_the_viewer_window_and_its_lock() {
        // no viewer window to read: the part is greyed and left out
        let mut w = Window::new();
        w.save_as("No window");
        assert_eq!(w.store().layout(0).and_then(|layout| layout.window.clone()), None);
        // with one: saved by default, locked when the box is ticked
        let mut w = Window::with_viewer();
        assert!(w.shows(tr!("fs-layouts-part-window")) && w.shows(tr!("fs-layouts-lock")));
        w.click(tr!("fs-layouts-lock"));
        w.changed = false;
        w.save_as("Locked");
        assert!(w.changed);
        assert_eq!(
            w.store().layout(0).and_then(|layout| layout.window.clone()),
            Some(WindowFrame::Normal {
                x: Some(100),
                y: Some(60),
                width: 1280,
                height: 900,
                maximized: false,
                locked: true,
            })
        );
        assert!(w.store().window_held);
        assert!(!w.seen().resizable, "locked at once");
        let words = format!(
            "{}, {}",
            tr!("fs-layouts-holds-window", width = 1280, height = 900),
            tr!("fs-layouts-holds-locked")
        );
        let texts = w.run(Vec::new());
        assert!(
            texts.iter().any(|t| t.source.starts_with(&words)),
            "what it holds starts with its window"
        );
    }

    #[test]
    fn unlock_is_greyed_until_a_layout_holds_the_window() {
        let mut w = Window::with_viewer();
        w.click(tr!("fs-layouts-unlock-button"));
        assert!(w.seen().resizable);
        w.click(tr!("fs-layouts-lock"));
        w.save_as("Locked");
        assert!(!w.seen().resizable);
        // (the row is below the fold with a layout selected: found by the search)
        w.st.query = "unlock".to_owned();
        w.settle();
        w.changed = false;
        w.click(tr!("fs-layouts-unlock-button"));
        w.run(Vec::new());
        w.st.query.clear();
        w.settle();
        assert!(w.changed);
        assert!(!w.store().window_held);
        assert!(w.seen().resizable);
        assert_eq!(
            w.viewer.as_ref().and_then(|viewer| viewer.log.last()),
            Some(&Command::Resizable(true))
        );
        // the layout keeps its lock box: applying it locks again
        w.click(tr!("fs-layouts-apply"));
        w.settle();
        assert!(w.store().window_held && !w.seen().resizable);
    }

    #[test]
    fn the_selected_layout_s_window_is_edited_in_the_tab() {
        let mut w = Window::with_viewer();
        w.save_as("Desk");
        assert_eq!(w.store().layout(0).map(|layout| layout.monitors.clone()), Some(two_monitors()));
        let window = |w: &Window| w.store().layout(0).and_then(|layout| layout.window.clone());
        // its lock box (the first of the two on the page): at once, for the layout in use
        w.click(tr!("fs-layouts-lock"));
        w.run(Vec::new());
        assert!(window(&w).is_some_and(|frame| frame.locked()));
        assert!(w.store().window_held && !w.seen().resizable);
        assert!(!w.shows(tr!("fs-layouts-modified")));
        // borderless: over the monitor the window is on, then over both
        w.click(tr!("fs-layouts-mode-borderless"));
        assert_eq!(
            window(&w),
            Some(WindowFrame::Borderless {
                monitors: vec![0],
                locked: true,
            })
        );
        assert!(w.shows(tr!("fs-layouts-modified")), "the viewer's window is not that yet");
        assert!(w.shows(tr!("fs-layouts-mode-pick-borderless")));
        w.click("2");
        assert_eq!(
            window(&w),
            Some(WindowFrame::Borderless {
                monitors: vec![0, 1],
                locked: true,
            })
        );
        // never without a monitor
        w.click("2");
        w.click("1");
        assert_eq!(
            window(&w),
            Some(WindowFrame::Borderless {
                monitors: vec![0],
                locked: true,
            })
        );
        w.click("2");
        // Apply does it
        w.click(tr!("fs-layouts-apply"));
        w.settle();
        assert!(!w.seen().decorated && !w.seen().resizable);
        assert_eq!((w.seen().outer, w.seen().inner), (Some((0, 0)), (3840, 1080)));
        assert!(!w.shows(tr!("fs-layouts-modified")));
        // full screen on the second monitor
        w.click(tr!("fs-layouts-mode-fullscreen"));
        assert_eq!(window(&w), Some(WindowFrame::Fullscreen { monitor: 0 }));
        assert!(w.shows(tr!("fs-layouts-mode-pick-fullscreen")));
        w.click("2");
        assert_eq!(window(&w), Some(WindowFrame::Fullscreen { monitor: 1 }));
        // and a normal window again: the viewer's own, as it is
        w.click(tr!("fs-layouts-mode-normal"));
        assert!(matches!(window(&w), Some(WindowFrame::Normal { .. })));
    }

    #[test]
    fn a_layout_saved_without_monitors_only_has_a_normal_window() {
        let mut w = Window::new();
        w.s.folderstorm_workspaces
            .add(
                Layout::new("Wayland", None, None, Vec::new()).with_window(Some(WindowFrame::Normal {
                    x: None,
                    y: None,
                    width: 1600,
                    height: 900,
                    maximized: false,
                    locked: false,
                })),
            )
            .expect("added");
        assert!(w.shows(tr!("fs-layouts-mode-no-monitors")));
        w.click(tr!("fs-layouts-mode-borderless"));
        assert!(matches!(
            w.store().layout(0).and_then(|layout| layout.window.clone()),
            Some(WindowFrame::Normal { .. })
        ));
    }

    #[test]
    fn what_happens_when_the_monitors_change_is_a_choice_of_three() {
        let mut w = Window::new();
        assert_eq!(w.store().on_monitor_change, MonitorPolicy::Ask);
        w.changed = false;
        w.click(tr!("fs-layouts-monitors-nothing"));
        assert_eq!(w.store().on_monitor_change, MonitorPolicy::Nothing);
        assert!(w.changed);
        w.click(tr!("fs-layouts-monitors-apply"));
        assert_eq!(w.store().on_monitor_change, MonitorPolicy::Apply);
        w.click(tr!("fs-layouts-monitors-ask"));
        assert_eq!(w.store().on_monitor_change, MonitorPolicy::Ask);
    }

    #[test]
    fn the_report_says_what_became_of_the_viewer_window() {
        let report = Report {
            name: Some("2 monitors".into()),
            windows: 3,
            fitted: 2,
            missing: 1,
            left: true,
            unlocked: true,
            ..Default::default()
        };
        let text = report_text(&report, 0);
        for part in [
            tr!("fs-layouts-applied-missing", count = 1),
            tr!("fs-layouts-applied-left").to_owned(),
            tr!("fs-layouts-applied-unlocked").to_owned(),
            tr!("fs-layouts-applied-fitted", count = 2),
        ] {
            assert!(text.contains(&part), "{text}");
        }
        let skipped = Report {
            no_window: true,
            ..Default::default()
        };
        assert!(report_text(&skipped, 0).contains(tr!("fs-layouts-applied-no-window")));
    }

    #[test]
    fn a_window_in_another_mode_keeps_what_it_can() {
        let saved = two_monitors();
        // a normal window on the right monitor
        let normal = WindowFrame::Normal {
            x: Some(2000),
            y: Some(100),
            width: 1600,
            height: 900,
            maximized: false,
            locked: true,
        };
        assert_eq!(
            in_mode(&normal, Mode::Fullscreen, &saved, None),
            WindowFrame::Fullscreen { monitor: 1 }
        );
        let borderless = in_mode(&normal, Mode::Borderless, &saved, None);
        assert_eq!(
            borderless,
            WindowFrame::Borderless {
                monitors: vec![1],
                locked: true,
            }
        );
        // back to a normal window: inside the monitors it covered
        assert_eq!(
            in_mode(&borderless, Mode::Normal, &saved, None),
            WindowFrame::Normal {
                x: Some(2112),
                y: Some(108),
                width: 1536,
                height: 864,
                maximized: false,
                locked: true,
            }
        );
        // or the viewer's own window when it can be read, with the layout's lock box
        let now = WindowFrame::Normal {
            x: Some(5),
            y: Some(6),
            width: 1000,
            height: 700,
            maximized: false,
            locked: false,
        };
        assert_eq!(in_mode(&borderless, Mode::Normal, &saved, Some(now.clone())), now.with_lock(true));
        // every result is a window the store takes
        for mode in [Mode::Normal, Mode::Fullscreen, Mode::Borderless] {
            for frame in [&normal, &borderless, &WindowFrame::Fullscreen { monitor: 0 }] {
                let layout = Layout::new("L", None, None, saved.clone()).with_window(Some(in_mode(frame, mode, &saved, None)));
                assert_eq!(layout.check(), Ok(()), "{frame:?} as {mode:?}");
            }
        }
        // a click on a monitor
        assert_eq!(with_monitor(&normal, 0), None);
        assert_eq!(with_monitor(&borderless, 1), None, "its last monitor stays");
        assert_eq!(
            with_monitor(&borderless, 0),
            Some(WindowFrame::Borderless {
                monitors: vec![0, 1],
                locked: true,
            })
        );
    }

    #[test]
    fn the_tab_gives_a_layout_its_star() {
        let mut w = Window::new();
        w.save_as("Wide");
        w.changed = false;
        // the star is the last of the selected layout's buttons, after the
        // two arrows
        let star = w.rect_of(tr!("common-delete")).expect("the actions").right_center() + vec2(85.0, 0.0);
        w.click_at(star);
        assert!(w.store().is_favorite("Wide"));
        assert!(w.changed, "the settings are saved");
        w.click_at(star);
        assert!(!w.store().is_favorite("Wide"));
    }

    #[test]
    fn the_two_layouts_of_the_toggle_are_chosen_in_the_tab() {
        let mut w = Window::new();
        w.save_as("Wide");
        w.save_as("Single");
        // the group is the last of the page, under the fold: found by the
        // window's search, which then shows its two rows alone
        w.st.query = "switcher".into();
        w.settle();
        assert!(w.shows(tr!("fs-layouts-toggle-first")) && w.shows(tr!("fs-layouts-toggle-second")));
        let all =
            |w: &mut Window, label: &str| -> Vec<Rect> { w.run(Vec::new()).iter().filter(|t| t.source == label).map(|t| t.rect).collect() };
        let unset = all(&mut w, tr!("fs-layouts-toggle-none"));
        assert_eq!(unset.len(), 2, "neither is chosen at first");
        // the first picker: its list opens over the page, "Wide" is in it
        w.click_at(unset[0].center());
        assert_eq!(all(&mut w, tr!("fs-layouts-toggle-none")).len(), 3, "the list is open");
        let wide = *all(&mut w, "Wide").last().expect("offered");
        w.changed = false;
        w.click_at(wide.center());
        assert_eq!(w.store().toggle, [Some("Wide".to_owned()), None]);
        assert!(w.changed, "the settings are saved");
        // the second one
        let unset = all(&mut w, tr!("fs-layouts-toggle-none"));
        assert_eq!(unset.len(), 1);
        w.click_at(unset[0].center());
        let single = *all(&mut w, "Single").last().expect("offered");
        w.click_at(single.center());
        assert_eq!(w.store().toggle, [Some("Wide".to_owned()), Some("Single".to_owned())]);
        // and back to none
        let first = *all(&mut w, "Wide").last().expect("the first picker shows it");
        w.click_at(first.center());
        let none = *all(&mut w, tr!("fs-layouts-toggle-none")).last().expect("offered");
        w.click_at(none.center());
        assert_eq!(w.store().toggle, [None, Some("Single".to_owned())]);
    }

    /// Draws Preferences on `tab` of the Layout page for a few frames with
    /// the software painter; the texts of the last frame.
    /// `FOLDERSTORM_TEST_CAPTURE_DIR=<folder>` keeps the picture as
    /// `<folder>/<name>.png`.
    fn drawn(name: &str, tab: usize, s: Settings, cogs: bool) -> Vec<String> {
        drawn_found(name, tab, s, cogs, "")
    }

    /// [`drawn`], with `query` typed in the window's search.
    fn drawn_found(name: &str, tab: usize, mut s: Settings, cogs: bool, query: &str) -> Vec<String> {
        let theme = crate::theme::Theme::default();
        let p = theme.skin_palette();
        let ctx = egui::Context::default();
        theme.apply(&ctx, 1.0);
        ctx.all_styles_mut(|s| s.animation_time = 0.0);
        let icons = crate::ui::icons::Icons::load(&ctx, None);
        let mut st = OptionsUi::default();
        st.open(Page::Layout, tab);
        st.demo_open_cogs = cogs;
        st.query = query.to_owned();
        let (width, height) = (1280u32, 900u32);
        let window = Rect::from_min_size(egui::Pos2::ZERO, vec2(width as f32, height as f32));
        let mut pixels = vec![0u32; (width * height) as usize];
        let mut painter = crate::crash::software::Painter::default();
        let mut texts = Vec::new();
        for frame in 0..8 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(window),
                    time: Some(frame as f64 / 60.0),
                    ..Default::default()
                },
                |ui| {
                    show(ui.ctx(), &p, &icons, &mut s, &mut st, &mut true);
                },
            );
            texts.clear();
            fn collect(shape: &egui::Shape, out: &mut Vec<String>) {
                match shape {
                    egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, out)),
                    egui::Shape::Text(t) => out.push(t.galley.text().to_owned()),
                    _ => {}
                }
            }
            for clipped in &output.shapes {
                collect(&clipped.shape, &mut texts);
            }
            let primitives = ctx.tessellate(std::mem::take(&mut output.shapes), output.pixels_per_point);
            pixels.fill(0);
            painter.paint(
                &mut pixels,
                [width as usize, height as usize],
                &primitives,
                &output.textures_delta,
                output.pixels_per_point,
                p.panel,
            );
            output.textures_delta.clear();
        }
        if let Some(dir) = std::env::var_os("FOLDERSTORM_TEST_CAPTURE_DIR") {
            let rgba: Vec<_> = pixels
                .iter()
                .flat_map(|p| [(*p >> 16) as u8, (*p >> 8) as u8, *p as u8, 255])
                .collect();
            let path = std::path::Path::new(&dir).join(format!("{name}.png"));
            image::save_buffer(path, &rgba, width, height, image::ColorType::Rgba8).expect("capture written");
        }
        texts
    }

    /// The two pickers of *Quick switch*, the last group of the tab, as
    /// the window's search shows them (`preferences-layouts-quick.png`).
    #[test]
    fn the_quick_switch_pickers_are_drawn() {
        let s = Settings {
            folderstorm_workspaces: serde_json::from_value(serde_json::json!({
                "layouts": [
                    Layout::new("2 monitors", Some(wide()), None, Vec::new()),
                    Layout::new("1 monitor", Some(Default::default()), None, Vec::new()),
                ],
                "favorites": [{ "layout": "1 monitor" }],
                "toggle": ["2 monitors", null],
            }))
            .expect("read"),
            ..Default::default()
        };
        let texts = drawn_found("preferences-layouts-quick", 3, s, false, "switcher");
        for expected in [
            tr!("fs-layouts-toggle-first"),
            tr!("fs-layouts-toggle-second"),
            "2 monitors",
            tr!("fs-layouts-toggle-none"),
        ] {
            assert!(texts.iter().any(|text| text == expected), "“{expected}” is drawn");
        }
    }

    /// The picture of the tab with a layout in use and modified, another
    /// one, and an entry from a newer version (`preferences-layouts.png`).
    #[test]
    fn the_tab_is_drawn_with_its_list_and_actions() {
        let s = Settings {
            folderstorm_workspaces: serde_json::from_value(serde_json::json!({
                "layouts": [
                    Layout::new("2 monitors", Some(wide()), None, Vec::new()),
                    Layout::new("1 monitor", Some(Default::default()), None, Vec::new()),
                    { "schema": 7, "name": "From a later version" },
                ],
                "active_layout": "2 monitors",
                "modified": true,
            }))
            .expect("read"),
            folderstorm_viewport: wide(),
            ..Default::default()
        };
        let texts = drawn("preferences-layouts", 3, s, false);
        for icon in [
            ICON_CHECK,
            ICON_SAVE,
            ICON_RENAME,
            ICON_DELETE,
            ICON_CANCEL,
            ICON_BACK,
            ICON_RESET,
            ICON_UP,
            ICON_DOWN,
            ICON_STAR,
        ] {
            assert!(crate::ui::icons::global(icon).is_some(), "the icon “{icon}” is embedded");
        }
        for expected in [
            "2 monitors",
            "1 monitor",
            "From a later version",
            tr!("fs-layouts-modified"),
            tr!("fs-layouts-newer"),
            tr!("fs-layouts-apply"),
            tr!("fs-layouts-save"),
            tr!("fs-layouts-previous-button"),
            tr!("fs-layouts-reset-button"),
        ] {
            assert!(texts.iter().any(|t| t == expected), "“{expected}” is drawn");
        }
    }

    /// The picture of the tab with a locked borderless layout selected:
    /// its window editor, the lock on its line, Unlock and the monitors'
    /// choice (`preferences-layouts-window.png`).
    #[test]
    fn the_tab_is_drawn_with_the_viewer_window_s_editor() {
        let s = Settings {
            folderstorm_workspaces: serde_json::from_value(serde_json::json!({
                "layouts": [
                    Layout::new("2 monitors", Some(wide()), None, two_monitors()).with_window(Some(WindowFrame::Borderless {
                        monitors: vec![0, 1],
                        locked: true,
                    })),
                    Layout::new("1 monitor", Some(Default::default()), None, two_monitors()[..1].to_vec()).with_window(Some(
                        WindowFrame::Normal {
                            x: Some(0),
                            y: Some(0),
                            width: 1920,
                            height: 1040,
                            maximized: false,
                            locked: false,
                        }
                    )),
                ],
                "active_layout": "2 monitors",
                "window_held": true,
            }))
            .expect("read"),
            folderstorm_viewport: wide(),
            ..Default::default()
        };
        assert!(s.folderstorm_workspaces.window_held);
        let texts = drawn("preferences-layouts-window", 3, s, false);
        for icon in [ICON_LOCK, ICON_UNLOCK] {
            assert!(crate::ui::icons::global(icon).is_some(), "the icon “{icon}” is embedded");
        }
        for expected in [
            tr!("fs-layouts-part-window"),
            tr!("fs-layouts-mode-normal"),
            tr!("fs-layouts-mode-fullscreen"),
            tr!("fs-layouts-mode-borderless"),
            tr!("fs-layouts-mode-pick-borderless"),
            tr!("fs-layouts-lock"),
            tr!("fs-layouts-unlock-button"),
            tr!("fs-layouts-monitors-ask"),
            tr!("fs-layouts-monitors-apply"),
            tr!("fs-layouts-monitors-nothing"),
        ] {
            assert!(texts.iter().any(|t| t == expected), "“{expected}” is drawn");
        }
    }

    /// The bars' three places are one choice of the World viewport group
    /// (`preferences-bars-place.png`).
    #[test]
    fn the_bars_place_is_one_choice_of_three() {
        let s = Settings {
            folderstorm_viewport: wide(),
            ..Default::default()
        };
        let texts = drawn("preferences-bars-place", 0, s, true);
        for expected in [
            tr!("folderstorm-viewport-bars"),
            tr!("folderstorm-viewport-bars-inside"),
            tr!("folderstorm-viewport-bars-around"),
            tr!("folderstorm-viewport-bars-window"),
        ] {
            assert!(texts.iter().any(|t| t == expected), "“{expected}” is drawn");
        }
    }
}
