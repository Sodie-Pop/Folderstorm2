//! Folderstorm: Preferences › Workspaces › Saved, a master–detail editor:
//! the list on the left (the workspaces, then the layouts, each line with
//! its kind's icon and its star; then *New from current…*), the selected
//! entry's editor on the right. Under the window's narrow width the list
//! stands above the editor.
//!
//! The actions are the same for both kinds (Apply, Update, Rename,
//! Delete, Move up and down, the star); Update and Delete ask first. A
//! workspace's editor shows the windows it opens and, beside its layout's
//! choice, a card of that layout; a layout's editor shows its viewer
//! window, a picture of what it gives on the monitors of now, its
//! background, and which workspaces and buttons use it. Layouts stay shared
//! between workspaces: "each workspace shows its layout" is shown, never
//! stored.

use super::super::super::cx::{Cx, Nothing};
use super::super::super::{OptionsUi, Page};
use super::super::folderstorm_search::words;
use super::{
    Ask, BUTTON_H, ICON_CANCEL, ICON_CHECK, ICON_DELETE, ICON_DOWN, ICON_LAYOUT, ICON_RENAME, ICON_SAVE, ICON_STAR, ICON_UP,
    ICON_WORKSPACE, Line, entry, muted, paint_icon, tab, warning, window_editor, workspace_editor,
};
use crate::folderstorm::workspaces::apply::{self, Keep, Live, Parts, Report};
use crate::folderstorm::workspaces::store::{MAX_NAME, StoreError, Unusable, WindowFrame, same_name};
use crate::folderstorm::workspaces::words::{self, refusal, report_text, workspace_holds, workspace_refusal};
use crate::folderstorm::workspaces::{preview, switcher, workspace};
use crate::settings::Settings;
use crate::theme::Palette;
use crate::ui::widgets::{self, ActionKind};
use aurora_i18n::tr;
use egui::{Align, Layout, Sense, vec2};

/// Under this width the list stands above the editor.
const TWO_PANES: f32 = 600.0;
/// Room inside a card, and between the two panes.
const PAD: f32 = 12.0;
const GAP: f32 = 16.0;
/// The embedded Tabler icons of the panes.
const ICON_NEW: &str = "plus";
const ICON_GO: &str = "arrow-right";

/// The two kinds of saved entries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Kind {
    #[default]
    Workspace,
    Layout,
}

/// An entry of the list: its kind and its place in its kind's list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Pick {
    pub kind: Kind,
    pub index: usize,
}

/// What was last refused, to say why under the field it came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Refused {
    Save(Kind, StoreError),
    Selected(Kind, StoreError),
}

fn refusal_text(kind: Kind, error: StoreError) -> String {
    match kind {
        Kind::Layout => refusal(error),
        Kind::Workspace => workspace_refusal(error),
    }
}

/// The tab's own state, for the session (egui's temporary data).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct State {
    /// The entry selected in the list.
    pub selected: Option<Pick>,
    /// *New from current…* is open in the right pane.
    new_form: bool,
    /// The name being typed for the selected entry.
    rename: Option<String>,
    ask: Option<Ask>,
    /// The question of a workspace's *Update* also updates its layout.
    also_layout: bool,
    refused: Option<Refused>,
    /// *Previous arrangement* was the last thing done (Switching).
    pub went_back: bool,
    /// The save form: what it saves, the name typed, and what it keeps.
    save_kind: Kind,
    new_name: String,
    parts: Parts,
    with_layout: bool,
    with_folders: bool,
}

impl Default for State {
    fn default() -> Self {
        State {
            selected: None,
            new_form: false,
            rename: None,
            ask: None,
            also_layout: false,
            refused: None,
            went_back: false,
            save_kind: Kind::Workspace,
            new_name: String::new(),
            parts: Parts::default(),
            with_layout: false,
            with_folders: false,
        }
    }
}

impl State {
    /// `pick` selected, the form and any question closed.
    fn select(&mut self, pick: Pick) {
        self.selected = Some(pick);
        self.new_form = false;
        self.rename = None;
        self.ask = None;
        self.refused = None;
    }
}

fn state_id() -> egui::Id {
    egui::Id::unique("folderstorm_saved_tab")
}

pub(super) fn load(ctx: &egui::Context) -> State {
    ctx.data(|d| d.get_temp::<State>(state_id())).unwrap_or_default()
}

pub(super) fn keep(ctx: &egui::Context, state: State) {
    ctx.data_mut(|d| d.insert_temp(state_id(), state));
}

fn rename_id() -> egui::Id {
    egui::Id::unique("folderstorm_saved_rename")
}

/// The rows of the tab: one, the two panes.
pub(super) fn page(cx: &mut Cx) {
    let p = cx.p;
    cx.group(tr!("fs-saved-group"), |cx| {
        cx.note(tr!("fs-saved-intro"));
        // (the search words of Folderstorm (C++): "dual monitor", "layout",
        // "workspace"; the broad ones on the list only)
        words(
            cx,
            &[
                "fs-search-layouts",
                "fs-search-workspaces",
                "fs-search-layouts-list",
                "fs-search-workspaces-list",
                "fs-search-layouts-background",
                "fs-search-layouts-save",
                "fs-search-workspaces-save",
            ],
        )
        .section_body()
        .row_with(tr!("fs-saved-list"), tr!("fs-saved-list-help"), &Nothing, |ui, s, opts, _| {
            panes(ui, &p, s, opts)
        });
    });
}

fn active_layout(s: &Settings) -> Option<usize> {
    let store = &s.folderstorm_workspaces;
    (0..store.layouts.len()).find(|index| store.is_active(*index))
}

fn active_workspace(s: &Settings) -> Option<usize> {
    let store = &s.folderstorm_workspaces;
    (0..store.workspaces.len()).find(|index| store.is_active_workspace(*index))
}

/// The entry in use, the workspace first: selected when nothing is.
fn in_use(s: &Settings) -> Option<Pick> {
    active_workspace(s)
        .map(|index| Pick {
            kind: Kind::Workspace,
            index,
        })
        .or_else(|| active_layout(s).map(|index| Pick { kind: Kind::Layout, index }))
}

fn count(s: &Settings, kind: Kind) -> usize {
    match kind {
        Kind::Layout => s.folderstorm_workspaces.layouts.len(),
        Kind::Workspace => s.folderstorm_workspaces.workspaces.len(),
    }
}

/// The entry's name, and whether this version can use it.
fn named(s: &Settings, pick: Pick) -> (String, bool) {
    let store = &s.folderstorm_workspaces;
    let (name, usable) = match pick.kind {
        Kind::Layout => store
            .layouts
            .get(pick.index)
            .map_or((None, false), |record| (record.name(), record.layout().is_some())),
        Kind::Workspace => store
            .workspaces
            .get(pick.index)
            .map_or((None, false), |record| (record.name(), record.workspace().is_some())),
    };
    (name.unwrap_or(tr!("fs-layouts-unnamed")).to_owned(), usable)
}

fn is_favorite(s: &Settings, pick: Pick, name: &str, usable: bool) -> bool {
    usable
        && match pick.kind {
            Kind::Layout => s.folderstorm_workspaces.is_favorite(name),
            Kind::Workspace => s.folderstorm_workspaces.is_favorite_workspace(name),
        }
}

/// The note at the right of an entry's line.
fn note(ctx: &egui::Context, s: &Settings, pick: Pick) -> &'static str {
    let store = &s.folderstorm_workspaces;
    let kept = match pick.kind {
        Kind::Layout => store.layouts.get(pick.index).and_then(|record| match record {
            crate::folderstorm::workspaces::store::Record::Kept(kept) => Some(kept.why),
            crate::folderstorm::workspaces::store::Record::Layout(_) => None,
        }),
        Kind::Workspace => store.workspaces.get(pick.index).and_then(|record| match record {
            workspace::Record::Kept(kept) => Some(kept.why),
            workspace::Record::Workspace(_) => None,
        }),
    };
    match kept {
        Some(Unusable::Newer) => return tr!("fs-layouts-newer"),
        Some(Unusable::Invalid) => return tr!("fs-layouts-invalid"),
        None => {}
    }
    let (active, modified) = match pick.kind {
        Kind::Layout => (store.is_active(pick.index), apply::modified(store, &s.folderstorm_viewport)),
        Kind::Workspace => (
            store.is_active_workspace(pick.index),
            apply::workspace_modified(store, &apply::open_now(ctx)),
        ),
    };
    match (active, modified) {
        (false, _) => "",
        (true, true) => tr!("fs-layouts-modified"),
        (true, false) => tr!("fs-layouts-active"),
    }
}

/// The entries in the list's order: the workspaces, then the layouts, each
/// in the user's order (a favorite keeps its place, with its star). With
/// each, the section it opens, if it opens one.
fn ordered(s: &Settings) -> Vec<(Option<&'static str>, Pick)> {
    let store = &s.folderstorm_workspaces;
    let workspaces = (0..store.workspaces.len()).map(|index| Pick {
        kind: Kind::Workspace,
        index,
    });
    let layouts = (0..store.layouts.len()).map(|index| Pick { kind: Kind::Layout, index });
    let mut out = Vec::new();
    for (title, picks) in [
        (tr!("fs-workspaces-list"), workspaces.collect::<Vec<_>>()),
        (tr!("fs-layouts-list"), layouts.collect()),
    ] {
        for (n, pick) in picks.into_iter().enumerate() {
            out.push(((n == 0).then_some(title), pick));
        }
    }
    out
}

/// A title in small capitals: a section of the list or of the editor.
fn section_title(ui: &mut egui::Ui, p: &Palette, title: &str) {
    ui.add_space(6.0);
    ui.label(egui::RichText::new(title.to_uppercase()).size(10.5).color(p.muted));
}

/// The list and the editor, side by side or one above the other. True
/// when the settings changed.
fn panes(ui: &mut egui::Ui, p: &Palette, s: &mut Settings, opts: &mut OptionsUi) -> bool {
    let ctx = ui.ctx().clone();
    let mut st = load(&ctx);
    let before = st.clone();
    if st.selected.is_some_and(|pick| pick.index >= count(s, pick.kind)) {
        st.selected = None;
    }
    if st.selected.is_none() {
        st.selected = in_use(s);
        st.rename = None;
        st.ask = None;
    }
    let mut changed = false;
    let width = ui.available_width();
    if width >= TWO_PANES {
        // (the editor's buttons on one line at the window's usual width)
        let list_w = (width * 0.3).clamp(180.0, 230.0);
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(vec2(list_w, 0.0), Layout::top_down(Align::Min), |ui| {
                ui.set_width(list_w);
                changed |= list_pane(ui, p, s, &mut st);
            });
            ui.add_space((GAP - ui.spacing().item_spacing.x).max(0.0));
            let rest = ui.available_width();
            ui.allocate_ui_with_layout(vec2(rest, 0.0), Layout::top_down(Align::Min), |ui| {
                ui.set_width(rest);
                changed |= right_pane(ui, p, s, &mut st, opts);
            });
        });
    } else {
        ui.vertical(|ui| {
            ui.set_width(width);
            changed |= list_pane(ui, p, s, &mut st);
            ui.add_space(GAP / 2.0);
            changed |= right_pane(ui, p, s, &mut st, opts);
        });
    }
    if st != before {
        keep(&ctx, st);
    }
    changed
}

/// The left pane: the entries, *New from current…*, a line of help.
fn list_pane(ui: &mut egui::Ui, p: &Palette, s: &mut Settings, st: &mut State) -> bool {
    let mut changed = false;
    ui.spacing_mut().item_spacing.y = 4.0;
    let entries = ordered(s);
    if entries.is_empty() {
        muted(ui, p, tr!("fs-saved-empty"));
    }
    for (title, pick) in entries {
        if let Some(title) = title {
            section_title(ui, p, title);
        }
        changed |= line(ui, p, s, st, pick);
    }
    ui.add_space(6.0);
    let width = ui.available_width();
    let kind = if st.new_form { ActionKind::Primary } else { ActionKind::Flat };
    if widgets::action_button(ui, p, ICON_NEW, tr!("fs-saved-new"), kind, vec2(width, BUTTON_H)).clicked() {
        st.new_form = true;
        st.rename = None;
        st.ask = None;
        st.refused = None;
    }
    ui.add_space(2.0);
    muted(ui, p, tr!("fs-saved-list-hint"));
    changed
}

/// One entry's line: a click selects it, a double click applies it.
fn line(ui: &mut egui::Ui, p: &Palette, s: &mut Settings, st: &mut State, pick: Pick) -> bool {
    let ctx = ui.ctx().clone();
    let (name, usable) = named(s, pick);
    let store = &s.folderstorm_workspaces;
    let (icon, active, locked) = match pick.kind {
        Kind::Layout => (
            ICON_LAYOUT,
            store.is_active(pick.index),
            store
                .layout(pick.index)
                .and_then(|layout| layout.window.as_ref())
                .is_some_and(WindowFrame::locked),
        ),
        Kind::Workspace => (ICON_WORKSPACE, store.is_active_workspace(pick.index), false),
    };
    let selected = st.selected == Some(pick) && !st.new_form;
    let response = entry(
        ui,
        p,
        &Line {
            icon,
            name: &name,
            selected,
            active,
            usable,
            locked,
            favorite: is_favorite(s, pick, &name, usable),
            note: note(&ctx, s, pick),
        },
    );
    if response.clicked() && !selected {
        st.select(pick);
    }
    // a double click applies, as Enter does in the switcher
    response.double_clicked() && usable && do_apply(&ctx, s, st, pick)
}

fn do_apply(ctx: &egui::Context, s: &mut Settings, st: &mut State, pick: Pick) -> bool {
    st.ask = None;
    st.rename = None;
    st.went_back = false;
    let done = match pick.kind {
        Kind::Layout => apply::apply(ctx, &mut Live::of(s), pick.index),
        Kind::Workspace => apply::apply_workspace(ctx, &mut Live::of(s), pick.index),
    };
    match done {
        Ok(_) => {
            st.refused = None;
            true
        }
        Err(error) => {
            st.refused = Some(Refused::Selected(pick.kind, error));
            false
        }
    }
}

/// Draws `body` on a card: the theme's corner, the `fill` given, room
/// around.
fn card<R>(ui: &mut egui::Ui, fill: egui::Color32, body: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let under = ui.painter().add(egui::Shape::Noop);
    let width = ui.available_width();
    let inner = (width - 2.0 * PAD).max(1.0);
    let out = ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::Min), |ui| {
        ui.set_width(width);
        ui.add_space(PAD);
        let out = ui
            .horizontal(|ui| {
                ui.add_space(PAD);
                ui.allocate_ui_with_layout(vec2(inner, 0.0), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(inner);
                    body(ui)
                })
                .inner
            })
            .inner;
        ui.add_space(PAD);
        out
    });
    let rect = out.response.rect;
    ui.painter()
        .set(under, egui::Shape::rect_filled(rect, ui.visuals().window_corner_radius, fill));
    out.inner
}

/// The right pane: the save form, or the selected entry's editor.
fn right_pane(ui: &mut egui::Ui, p: &Palette, s: &mut Settings, st: &mut State, opts: &mut OptionsUi) -> bool {
    let nothing = s.folderstorm_workspaces.layouts.is_empty() && s.folderstorm_workspaces.workspaces.is_empty();
    card(ui, p.field, |ui| {
        if st.new_form || nothing {
            new_form(ui, p, s, st)
        } else if let Some(pick) = st.selected {
            detail(ui, p, s, st, pick, opts)
        } else {
            muted(ui, p, tr!("fs-saved-pick"));
            false
        }
    })
}

/// Whether the last apply was this entry's.
fn report_of(report: &Report, kind: Kind, name: &str) -> bool {
    match kind {
        Kind::Workspace => report.workspace.as_deref() == Some(name),
        Kind::Layout => report.workspace.is_none() && report.name.as_deref() == Some(name),
    }
}

/// The editor's head: the kind's icon, the name, its kind and state.
fn head(ui: &mut egui::Ui, p: &Palette, icon: &str, name: &str, kind: &str, state: &str) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
        paint_icon(ui, icon, rect.center(), 16.0, if state.is_empty() { p.ink } else { p.teal });
        ui.add(egui::Label::new(egui::RichText::new(name).size(15.0).strong().color(p.ink)).truncate());
        ui.label(egui::RichText::new(kind).size(11.5).color(p.muted));
        if !state.is_empty() {
            ui.label(egui::RichText::new(state).size(11.5).color(p.teal));
        }
    });
}

/// The selected entry: its head, what it holds, its actions, what its last
/// apply did, and its kind's editor. True when the settings changed.
fn detail(ui: &mut egui::Ui, p: &Palette, s: &mut Settings, st: &mut State, pick: Pick, opts: &mut OptionsUi) -> bool {
    let ctx = ui.ctx().clone();
    let (name, usable) = named(s, pick);
    let (icon, kind) = match pick.kind {
        Kind::Workspace => (ICON_WORKSPACE, tr!("fs-workspaces-save-workspace")),
        Kind::Layout => (ICON_LAYOUT, tr!("fs-workspaces-save-layout")),
    };
    let mut changed = false;
    head(ui, p, icon, &name, kind, note(&ctx, s, pick));
    holds(ui, p, s, pick);
    ui.add_space(4.0);
    changed |= actions(ui, p, s, st, pick, &name, usable);
    if let Some(Refused::Selected(kind, error)) = st.refused
        && kind == pick.kind
    {
        warning(ui, p, refusal_text(kind, error));
    }
    // the last apply, in one line; its details on hover
    if let Some(report) = apply::report(&ctx).filter(|report| report_of(report, pick.kind, &name)) {
        let text = report_text(&report, s.folderstorm_workspaces.pending.len());
        let first = text.split_inclusive(". ").next().unwrap_or(&text).trim_end().to_owned();
        let shown = ui.add(egui::Label::new(egui::RichText::new(&first).size(11.5).color(p.muted)).truncate());
        if first != text {
            shown.on_hover_text(text);
        }
    }
    if st.rename.is_none() && st.ask.is_none() && usable {
        changed |= match pick.kind {
            Kind::Workspace => workspace_detail(ui, p, s, st, pick.index, &name),
            Kind::Layout => layout_detail(ui, p, s, st, pick.index, &name, opts),
        };
    }
    changed
}

/// What the entry holds, or why it cannot be used.
fn holds(ui: &mut egui::Ui, p: &Palette, s: &Settings, pick: Pick) {
    let store = &s.folderstorm_workspaces;
    let text = match pick.kind {
        Kind::Layout => match store.layouts.get(pick.index) {
            Some(crate::folderstorm::workspaces::store::Record::Layout(layout)) => words::holds(layout),
            Some(crate::folderstorm::workspaces::store::Record::Kept(kept)) => match kept.why {
                Unusable::Newer => tr!("fs-layouts-newer-help").to_owned(),
                Unusable::Invalid => tr!("fs-layouts-invalid-help").to_owned(),
            },
            None => return,
        },
        Kind::Workspace => match store.workspaces.get(pick.index) {
            Some(workspace::Record::Workspace(workspace)) => workspace_holds(workspace),
            Some(workspace::Record::Kept(kept)) => match kept.why {
                Unusable::Newer => tr!("fs-workspaces-newer-help").to_owned(),
                Unusable::Invalid => tr!("fs-workspaces-invalid-help").to_owned(),
            },
            None => return,
        },
    };
    if !text.is_empty() {
        muted(ui, p, text);
    }
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

/// A name being typed, a question being asked, or the buttons: the same
/// for both kinds. True when the settings changed.
fn actions(ui: &mut egui::Ui, p: &Palette, s: &mut Settings, st: &mut State, pick: Pick, name: &str, usable: bool) -> bool {
    let ctx = ui.ctx().clone();
    let size = vec2(0.0, BUTTON_H);
    let mut changed = false;
    if let Some(typed) = st.rename.as_mut() {
        let mut done = None;
        ui.horizontal(|ui| {
            let field = ui.add(
                egui::TextEdit::singleline(typed)
                    .id(rename_id())
                    .desired_width(240.0)
                    .char_limit(MAX_NAME * 2),
            );
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if widgets::action_button(ui, p, ICON_CHECK, tr!("fs-layouts-rename"), ActionKind::Primary, size).clicked() || enter {
                done = Some(true);
            }
            if widgets::action_button(ui, p, ICON_CANCEL, tr!("common-cancel"), ActionKind::Flat, size).clicked() {
                done = Some(false);
            }
        });
        match done {
            Some(true) => {
                let store = &mut s.folderstorm_workspaces;
                let renamed = match pick.kind {
                    Kind::Layout => store.rename(pick.index, typed),
                    Kind::Workspace => store.rename_workspace(pick.index, typed),
                };
                match renamed {
                    Ok(()) => {
                        st.rename = None;
                        st.refused = None;
                        changed = true;
                    }
                    Err(error) => st.refused = Some(Refused::Selected(pick.kind, error)),
                }
            }
            Some(false) => {
                st.rename = None;
                st.refused = None;
            }
            None => {}
        }
        return changed;
    }
    if let Some(ask) = st.ask {
        let question = match (ask, pick.kind) {
            (Ask::Update, Kind::Layout) => tr!("fs-layouts-ask-update", name = name),
            (Ask::Update, Kind::Workspace) => tr!("fs-workspaces-ask-update", name = name),
            (Ask::Delete, Kind::Layout) => tr!("fs-layouts-ask-delete", name = name),
            (Ask::Delete, Kind::Workspace) => tr!("fs-workspaces-ask-delete", name = name),
        };
        let (icon, label, kind) = match ask {
            Ask::Update => (ICON_SAVE, tr!("fs-layouts-update"), ActionKind::Primary),
            Ask::Delete => (ICON_DELETE, tr!("common-delete"), ActionKind::Danger),
        };
        ui.add(egui::Label::new(egui::RichText::new(question).size(12.5).color(p.ink)).wrap());
        let offer = match (ask, pick.kind) {
            (Ask::Update, Kind::Workspace) => apply::layout_offer(&s.folderstorm_workspaces, &s.folderstorm_viewport, pick.index),
            _ => None,
        };
        if let Some((layout, _)) = &offer {
            widgets::checkbox(
                ui,
                p,
                &mut st.also_layout,
                tr!("fs-workspaces-update-layout", name = layout.as_str()),
            );
        }
        let mut answer = None;
        ui.horizontal(|ui| {
            if widgets::action_button(ui, p, icon, label, kind, size).clicked() {
                answer = Some(true);
            }
            if widgets::action_button(ui, p, ICON_CANCEL, tr!("common-cancel"), ActionKind::Flat, size).clicked() {
                answer = Some(false);
            }
        });
        if answer == Some(true) {
            st.went_back = false;
            match ask {
                Ask::Update => {
                    let updated = match pick.kind {
                        Kind::Layout => apply::update(&ctx, &mut Live::of(s), pick.index),
                        Kind::Workspace => {
                            let also_layout = offer.is_some() && st.also_layout;
                            apply::update_workspace_with_layout(&ctx, &mut Live::of(s), pick.index, also_layout)
                        }
                    };
                    match updated {
                        Ok(()) => {
                            st.refused = None;
                            changed = true;
                        }
                        Err(error) => st.refused = Some(Refused::Selected(pick.kind, error)),
                    }
                }
                Ask::Delete => {
                    let store = &mut s.folderstorm_workspaces;
                    changed |= match pick.kind {
                        Kind::Layout => store.remove(pick.index),
                        Kind::Workspace => store.remove_workspace(pick.index),
                    };
                    st.selected = None;
                    st.refused = None;
                }
            }
        }
        if answer.is_some() {
            st.ask = None;
        }
        return changed;
    }
    let count = count(s, pick.kind);
    let favorite = is_favorite(s, pick, name, usable);
    ui.horizontal_wrapped(|ui| {
        if widgets::action_button_if(ui, p, ICON_CHECK, tr!("fs-layouts-apply"), ActionKind::Primary, size, usable).clicked() {
            changed |= do_apply(&ctx, s, st, pick);
        }
        let update_help = match pick.kind {
            Kind::Layout => tr!("fs-switcher-update-help"),
            Kind::Workspace => tr!("fs-workspaces-update-help"),
        };
        if widgets::action_button_if(ui, p, ICON_SAVE, tr!("fs-layouts-update"), ActionKind::Flat, size, usable)
            .on_hover_text(update_help)
            .clicked()
        {
            st.ask = Some(Ask::Update);
            st.also_layout = pick.kind == Kind::Workspace
                && apply::layout_offer(&s.folderstorm_workspaces, &s.folderstorm_viewport, pick.index).is_some_and(|(_, ticked)| ticked);
        }
        if widgets::action_button_if(ui, p, ICON_RENAME, tr!("fs-layouts-rename"), ActionKind::Flat, size, usable).clicked() {
            start_rename(&ctx, name);
            st.rename = Some(name.to_owned());
        }
        if widgets::action_button(ui, p, ICON_DELETE, tr!("common-delete"), ActionKind::FlatDanger, size).clicked() {
            st.ask = Some(Ask::Delete);
        }
        let store = &mut s.folderstorm_workspaces;
        let shift = |store: &mut crate::folderstorm::workspaces::store::Store, up: bool| match pick.kind {
            Kind::Layout => store.shift(pick.index, up),
            Kind::Workspace => store.shift_workspace(pick.index, up),
        };
        if widgets::icon_button(ui, p, ICON_UP, tr!("fs-layouts-move-up"), pick.index > 0)
            && let Some(to) = shift(store, true)
        {
            st.selected = Some(Pick { index: to, ..pick });
            changed = true;
        }
        if widgets::icon_button(ui, p, ICON_DOWN, tr!("fs-layouts-move-down"), pick.index + 1 < count)
            && let Some(to) = shift(store, false)
        {
            st.selected = Some(Pick { index: to, ..pick });
            changed = true;
        }
        // a favorite: in the strip, first in the switcher and in World ›
        // Layouts, and in the keyboard's round
        let tip = if favorite {
            tr!("fs-switcher-unstar")
        } else {
            tr!("fs-switcher-star")
        };
        if usable && widgets::icon_toggle(ui, p, ICON_STAR, tip, favorite) {
            let store = &mut s.folderstorm_workspaces;
            changed |= match pick.kind {
                Kind::Layout => store.set_favorite(name, !favorite),
                Kind::Workspace => store.set_favorite_workspace(name, !favorite),
            };
        }
    });
    changed
}

/// A workspace's editor: the windows it opens, where they go (its layout
/// and that layout's card), its graphics, camera and HUDs.
fn workspace_detail(ui: &mut egui::Ui, p: &Palette, s: &mut Settings, st: &mut State, index: usize, name: &str) -> bool {
    section_title(ui, p, tr!("fs-saved-section-windows"));
    let mut changed = workspace_editor::window_boxes(ui, p, s, index, name);
    changed |= workspace_editor::folders_box(ui, p, s, index);
    section_title(ui, p, tr!("fs-saved-section-where"));
    changed |= workspace_editor::layout_picker(ui, s, index);
    layout_card(ui, p, s, st, index);
    ui.add_space(4.0);
    changed |= workspace_editor::context_section(ui, p, s, index, name);
    changed
}

/// The card of the workspace's layout, shown, not stored: its name, *Open
/// this layout*, the picture of what it gives on the monitors of now, and
/// what it holds. "Keep the layout in use" says so instead.
fn layout_card(ui: &mut egui::Ui, p: &Palette, s: &Settings, st: &mut State, index: usize) {
    let store = &s.folderstorm_workspaces;
    let found = store
        .workspace(index)
        .and_then(|workspace| workspace.layout.as_deref())
        .and_then(|name| store.find(name));
    let Some((at, layout)) = found else {
        muted(ui, p, tr!("fs-workspaces-holds-no-layout"));
        return;
    };
    let ctx = ui.ctx().clone();
    let picture = preview::of_now(&ctx, layout, &s.folderstorm_viewport);
    let mut open = false;
    card(ui, p.raised, |ui| {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
            paint_icon(ui, ICON_LAYOUT, rect.center(), 14.0, p.ink);
            ui.label(egui::RichText::new(&layout.name).size(13.0).strong().color(p.ink));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                open = widgets::action_button(ui, p, ICON_GO, tr!("fs-saved-open-layout"), ActionKind::Flat, vec2(0.0, BUTTON_H)).clicked();
            });
        });
        preview::show(ui, p, &picture).on_hover_text(tr!("fs-switcher-picture-help"));
        muted(ui, p, words::holds(layout));
        for line in switcher::notes(&picture.notes) {
            warning(ui, p, line);
        }
    });
    if open {
        st.select(Pick {
            kind: Kind::Layout,
            index: at,
        });
    }
}

/// A layout's editor: its viewer window, the picture of its viewport and
/// bars, its background, and what uses it.
#[allow(clippy::too_many_arguments)]
fn layout_detail(ui: &mut egui::Ui, p: &Palette, s: &mut Settings, st: &mut State, index: usize, name: &str, opts: &mut OptionsUi) -> bool {
    let ctx = ui.ctx().clone();
    let mut changed = false;
    let Some(layout) = s.folderstorm_workspaces.layout(index).cloned() else {
        return false;
    };
    section_title(ui, p, tr!("fs-layouts-part-window"));
    match &layout.window {
        Some(frame) => {
            if let Some(edited) = window_editor(ui, p, &layout, frame, apply::viewer_window(&ctx)) {
                match apply::set_window(&mut s.folderstorm_workspaces, index, edited) {
                    Ok(()) => {
                        st.refused = None;
                        changed = true;
                    }
                    Err(error) => st.refused = Some(Refused::Selected(Kind::Layout, error)),
                }
            }
        }
        None => muted(ui, p, tr!("fs-saved-no-window")),
    }
    section_title(ui, p, tr!("fs-layouts-part-world"));
    let picture = preview::of_now(&ctx, &layout, &s.folderstorm_viewport);
    preview::show(ui, p, &picture).on_hover_text(tr!("fs-switcher-picture-help"));
    for line in switcher::notes(&picture.notes) {
        warning(ui, p, line);
    }
    if widgets::action_button(ui, p, ICON_GO, tr!("fs-saved-edit-viewport"), ActionKind::Flat, vec2(0.0, BUTTON_H)).clicked() {
        opts.go_to = Some((Page::Workspaces, tab::VIEWPORT, None));
    }
    // its own background, or the default (row 1.8)
    ui.add_space(4.0);
    changed |= super::super::folderstorm_viewport::background::layout_part(ui, p, s, index);
    section_title(ui, p, tr!("fs-saved-used-by"));
    used_by(ui, p, s, st, name, opts);
    changed
}

/// Which workspaces apply the layout `name`, and whether the *Switch
/// layout* button uses it: a link the list never showed.
fn used_by(ui: &mut egui::Ui, p: &Palette, s: &Settings, st: &mut State, name: &str, opts: &mut OptionsUi) {
    let store = &s.folderstorm_workspaces;
    let users: Vec<(usize, String)> = store
        .workspaces
        .iter()
        .enumerate()
        .filter_map(|(at, record)| {
            let workspace = record.workspace()?;
            workspace
                .layout
                .as_deref()
                .is_some_and(|layout| same_name(layout, name))
                .then(|| (at, workspace.name.clone()))
        })
        .collect();
    if users.is_empty() {
        muted(ui, p, tr!("fs-saved-used-by-none"));
    } else {
        muted(ui, p, tr!("fs-saved-used-by-some"));
        let mut picked = None;
        ui.horizontal_wrapped(|ui| {
            for (at, user) in &users {
                if widgets::action_button(ui, p, ICON_WORKSPACE, user, ActionKind::Flat, vec2(0.0, BUTTON_H)).clicked() {
                    picked = Some(*at);
                }
            }
        });
        if let Some(at) = picked {
            st.select(Pick {
                kind: Kind::Workspace,
                index: at,
            });
        }
    }
    let toggle: Vec<&str> = [tr!("fs-saved-toggle-first"), tr!("fs-saved-toggle-second")]
        .into_iter()
        .zip(&store.toggle)
        .filter(|(_, chosen)| chosen.as_deref().is_some_and(|chosen| same_name(chosen, name)))
        .map(|(text, _)| text)
        .collect();
    for text in toggle {
        muted(ui, p, text);
    }
    if widgets::action_button(ui, p, ICON_GO, tr!("fs-saved-switching"), ActionKind::Flat, vec2(0.0, BUTTON_H)).clicked() {
        opts.go_to = Some((Page::Workspaces, tab::SWITCHING, None));
    }
}

/// *New from current…*: its head, then the save form. True when the
/// settings changed.
fn new_form(ui: &mut egui::Ui, p: &Palette, s: &mut Settings, st: &mut State) -> bool {
    ui.label(egui::RichText::new(tr!("fs-saved-new")).size(15.0).strong().color(p.ink));
    muted(ui, p, tr!("fs-saved-save-help"));
    ui.add_space(4.0);
    save_form(ui, p, s, st)
}

/// The save form: a workspace or a layout, its name, what it keeps, Save.
/// True when the settings changed.
fn save_form(ui: &mut egui::Ui, p: &Palette, s: &mut Settings, st: &mut State) -> bool {
    let ctx = ui.ctx().clone();
    let mut changed = false;
    let mut save = false;
    ui.horizontal_wrapped(|ui| {
        let kinds = [
            (Kind::Workspace, tr!("fs-workspaces-save-workspace"), false),
            (Kind::Layout, tr!("fs-workspaces-save-layout"), false),
        ];
        if super::super::super::segmented(ui, p, &mut st.save_kind, &kinds) {
            st.refused = None;
        }
        let hint = match st.save_kind {
            Kind::Workspace => tr!("fs-workspaces-name-hint"),
            Kind::Layout => tr!("fs-layouts-name-hint"),
        };
        let field = ui.add(
            egui::TextEdit::singleline(&mut st.new_name)
                .desired_width(220.0)
                .char_limit(MAX_NAME * 2)
                .hint_text(hint),
        );
        save |= field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let help = match st.save_kind {
            Kind::Workspace => tr!("fs-workspaces-save-help"),
            Kind::Layout => tr!("fs-layouts-save-new-help"),
        };
        save |= widgets::action_button(ui, p, ICON_SAVE, tr!("fs-layouts-save"), ActionKind::Primary, vec2(0.0, BUTTON_H))
            .on_hover_text(help)
            .clicked();
    });
    let no_parts = match st.save_kind {
        Kind::Workspace => {
            keeps_workspace(ui, p, s, st);
            false
        }
        Kind::Layout => keeps_layout(ui, p, st),
    };
    if no_parts {
        warning(ui, p, tr!("fs-layouts-error-parts"));
    } else if save {
        let saved = match st.save_kind {
            Kind::Layout => apply::save_new(&ctx, &mut Live::of(s), &st.new_name, st.parts),
            Kind::Workspace => {
                let keep = Keep {
                    layout: st.with_layout && s.folderstorm_workspaces.active().is_some(),
                    folders: st.with_folders,
                };
                apply::save_new_workspace(&ctx, &mut Live::of(s), &st.new_name, keep)
            }
        };
        match saved {
            Ok(index) => {
                st.new_name.clear();
                st.select(Pick { kind: st.save_kind, index });
                st.went_back = false;
                changed = true;
            }
            Err(error) => st.refused = Some(Refused::Save(st.save_kind, error)),
        }
    }
    if let Some(Refused::Save(kind, error)) = st.refused
        && kind == st.save_kind
    {
        warning(ui, p, refusal_text(kind, error));
    }
    changed
}

/// "It keeps:" for a workspace: the layout in use, the Inventory folders.
fn keeps_workspace(ui: &mut egui::Ui, p: &Palette, s: &Settings, st: &mut State) {
    let layout = s.folderstorm_workspaces.active().map(|layout| layout.name.clone());
    ui.horizontal_wrapped(|ui| {
        muted(ui, p, tr!("fs-saved-keeps"));
        ui.add_enabled_ui(layout.is_some(), |ui| {
            let mut with = st.with_layout && layout.is_some();
            let label = match &layout {
                Some(name) => tr!("fs-workspaces-with-layout", name = name.as_str()),
                None => tr!("fs-workspaces-with-no-layout").to_owned(),
            };
            if widgets::checkbox(ui, p, &mut with, label).changed() {
                st.with_layout = with;
            }
        });
        widgets::checkbox(ui, p, &mut st.with_folders, tr!("fs-workspaces-remember-folders"))
            .on_hover_text(tr!("fs-workspaces-remember-folders-help"));
    });
}

/// "It keeps:" for a layout: its parts and the lock. True when none is
/// ticked (nothing to save).
fn keeps_layout(ui: &mut egui::Ui, p: &Palette, st: &mut State) -> bool {
    // (no viewer window to read under a forced size or a capture)
    let readable = apply::has_viewer_window(ui.ctx());
    ui.horizontal_wrapped(|ui| {
        muted(ui, p, tr!("fs-saved-keeps"));
        ui.add_enabled_ui(readable, |ui| {
            let mut window = st.parts.window && readable;
            if widgets::checkbox(ui, p, &mut window, tr!("fs-layouts-part-window")).changed() {
                st.parts.window = window;
            }
        });
        widgets::checkbox(ui, p, &mut st.parts.world, tr!("fs-layouts-part-world"));
        widgets::checkbox(ui, p, &mut st.parts.windows, tr!("fs-layouts-part-windows"));
    });
    let with_window = st.parts.window && readable;
    ui.add_enabled_ui(with_window, |ui| {
        let mut lock = st.parts.lock && with_window;
        if widgets::checkbox(ui, p, &mut lock, tr!("fs-layouts-lock")).changed() {
            st.parts.lock = lock;
        }
    });
    !with_window && !st.parts.world && !st.parts.windows
}
