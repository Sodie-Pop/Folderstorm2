//! Folderstorm: Preferences › Workspaces › Saved: the workspaces and the
//! layouts in one list, in the switcher's order (favorites, workspaces,
//! layouts), each line with its kind's icon; the selected entry's actions
//! and editor under its line; one form that saves either kind.
//!
//! The actions are the same for both kinds (Apply, Update, Rename,
//! Delete, Move up and down, the star); Update and Delete ask first. A
//! layout's editor is its viewer window and its background, a
//! workspace's is in [`super::workspace_editor`].

use super::super::super::cx::{Cx, Nothing};
use super::super::folderstorm_search::words;
use super::{
    Ask, BUTTON_H, ICON_CANCEL, ICON_CHECK, ICON_DELETE, ICON_DOWN, ICON_LAYOUT, ICON_RENAME, ICON_SAVE, ICON_STAR, ICON_UP,
    ICON_WORKSPACE, LIST_W, Line, entry, muted, warning, window_editor, workspace_editor,
};
use crate::folderstorm::workspaces::apply::{self, Keep, Live, Parts, Report};
use crate::folderstorm::workspaces::store::{MAX_NAME, StoreError, Unusable, WindowFrame};
use crate::folderstorm::workspaces::words::{self, refusal, report_text, workspace_holds, workspace_refusal};
use crate::folderstorm::workspaces::workspace;
use crate::settings::Settings;
use crate::theme::Palette;
use crate::ui::widgets::{self, ActionKind};
use aurora_i18n::tr;
use egui::vec2;

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
    /// The name being typed for the selected entry.
    rename: Option<String>,
    ask: Option<Ask>,
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
            rename: None,
            ask: None,
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

/// The rows of the tab: the list, then the save form.
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
            ],
        )
        .section_body()
        .row(tr!("fs-saved-list"), tr!("fs-saved-list-help"), &Nothing, |ui, s| list(ui, &p, s));
    });
    cx.group(tr!("fs-saved-save-group"), |cx| {
        words(cx, &["fs-search-layouts-save", "fs-search-workspaces-save"])
            .section_body()
            .row(tr!("fs-saved-save"), tr!("fs-saved-save-help"), &Nothing, |ui, s| {
                save_form(ui, &p, s)
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

/// The entries in the list's order: the favorites first, in their own
/// order, then the other workspaces, then the other layouts. With each,
/// the section it opens, if it opens one.
fn ordered(s: &Settings) -> Vec<(Option<&'static str>, Pick)> {
    let store = &s.folderstorm_workspaces;
    let mut out: Vec<(Option<&'static str>, Pick)> = Vec::new();
    let mut favorites = Vec::new();
    for favorite in &store.favorites {
        let pick = if let Some(name) = favorite.workspace() {
            store.find_workspace(name).map(|(index, _)| Pick {
                kind: Kind::Workspace,
                index,
            })
        } else {
            favorite
                .layout()
                .and_then(|name| store.find(name))
                .map(|(index, _)| Pick { kind: Kind::Layout, index })
        };
        if let Some(pick) = pick.filter(|pick| !favorites.contains(pick)) {
            favorites.push(pick);
        }
    }
    let section = |out: &mut Vec<(Option<&'static str>, Pick)>, title: &'static str, picks: Vec<Pick>| {
        for (n, pick) in picks.into_iter().enumerate() {
            out.push(((n == 0).then_some(title), pick));
        }
    };
    let workspaces = (0..store.workspaces.len())
        .map(|index| Pick {
            kind: Kind::Workspace,
            index,
        })
        .filter(|pick| !favorites.contains(pick))
        .collect();
    let layouts = (0..store.layouts.len())
        .map(|index| Pick { kind: Kind::Layout, index })
        .filter(|pick| !favorites.contains(pick))
        .collect();
    section(&mut out, tr!("fs-switcher-favorites"), favorites);
    section(&mut out, tr!("fs-workspaces-list"), workspaces);
    section(&mut out, tr!("fs-layouts-list"), layouts);
    out
}

/// A section's title in the list, in small capitals.
fn section_title(ui: &mut egui::Ui, p: &Palette, title: &str) {
    ui.add_space(4.0);
    ui.label(egui::RichText::new(title.to_uppercase()).size(10.5).color(p.muted));
}

/// The list, the selected entry's actions and editor under its line. True
/// when the settings changed.
fn list(ui: &mut egui::Ui, p: &Palette, s: &mut Settings) -> bool {
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
    ui.vertical(|ui| {
        ui.set_width(ui.available_width().min(LIST_W));
        ui.spacing_mut().item_spacing.y = 4.0;
        let entries = ordered(s);
        if entries.is_empty() {
            muted(ui, p, tr!("fs-saved-empty"));
        }
        for (title, pick) in entries {
            if let Some(title) = title {
                section_title(ui, p, title);
            }
            changed |= line(ui, p, s, &mut st, pick);
            if st.selected == Some(pick) {
                changed |= detail(ui, p, s, &mut st, pick);
            }
        }
    });
    if st != before {
        keep(&ctx, st);
    }
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
    let selected = st.selected == Some(pick);
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
        st.selected = Some(pick);
        st.rename = None;
        st.ask = None;
        st.refused = None;
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

/// Whether the last apply was this entry's.
fn report_of(report: &Report, kind: Kind, name: &str) -> bool {
    match kind {
        Kind::Workspace => report.workspace.as_deref() == Some(name),
        Kind::Layout => report.workspace.is_none() && report.name.as_deref() == Some(name),
    }
}

/// Under the selected line: what the entry holds, its actions, what its
/// last apply did, and its editor. True when the settings changed.
fn detail(ui: &mut egui::Ui, p: &Palette, s: &mut Settings, st: &mut State, pick: Pick) -> bool {
    let ctx = ui.ctx().clone();
    let (name, usable) = named(s, pick);
    let mut changed = false;
    ui.add_space(2.0);
    // the editor sits a little inside the list, under its line
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.vertical(|ui| {
            holds(ui, p, s, pick);
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
                changed |= editor(ui, p, s, st, pick, &name);
            }
        });
    });
    ui.add_space(6.0);
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
    muted(ui, p, text);
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
                        Kind::Workspace => apply::update_workspace(&ctx, &mut Live::of(s), pick.index),
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
        }
        if widgets::action_button_if(ui, p, ICON_RENAME, tr!("fs-layouts-rename"), ActionKind::Flat, size, usable).clicked() {
            start_rename(&ctx, name);
            st.rename = Some(name.to_owned());
        }
        if widgets::action_button(ui, p, ICON_DELETE, tr!("common-delete"), ActionKind::FlatDanger, size).clicked() {
            st.ask = Some(Ask::Delete);
        }
        // (a favorite's place is the favorites' order: its arrows greyed)
        let store = &mut s.folderstorm_workspaces;
        let shift = |store: &mut crate::folderstorm::workspaces::store::Store, up: bool| match pick.kind {
            Kind::Layout => store.shift(pick.index, up),
            Kind::Workspace => store.shift_workspace(pick.index, up),
        };
        if widgets::icon_button(ui, p, ICON_UP, tr!("fs-layouts-move-up"), !favorite && pick.index > 0)
            && let Some(to) = shift(store, true)
        {
            st.selected = Some(Pick { index: to, ..pick });
            changed = true;
        }
        if widgets::icon_button(ui, p, ICON_DOWN, tr!("fs-layouts-move-down"), !favorite && pick.index + 1 < count)
            && let Some(to) = shift(store, false)
        {
            st.selected = Some(Pick { index: to, ..pick });
            changed = true;
        }
        // a favorite: first here, in the switcher, in World › Layouts, in
        // the strip and in the keyboard's round
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

/// The kind's own editor: a layout's viewer window and background, a
/// workspace's layout, windows, folders and context.
fn editor(ui: &mut egui::Ui, p: &Palette, s: &mut Settings, st: &mut State, pick: Pick, name: &str) -> bool {
    let ctx = ui.ctx().clone();
    match pick.kind {
        Kind::Workspace => workspace_editor::editor(ui, p, s, pick.index, name),
        Kind::Layout => {
            let mut changed = false;
            let layout = s.folderstorm_workspaces.layout(pick.index).cloned();
            if let Some(layout) = &layout
                && let Some(frame) = &layout.window
                && let Some(edited) = window_editor(ui, p, layout, frame, apply::viewer_window(&ctx))
            {
                match apply::set_window(&mut s.folderstorm_workspaces, pick.index, edited) {
                    Ok(()) => {
                        st.refused = None;
                        changed = true;
                    }
                    Err(error) => st.refused = Some(Refused::Selected(Kind::Layout, error)),
                }
            }
            // its own background, or the default (row 1.8)
            changed |= super::super::folderstorm_viewport::background::layout_part(ui, p, s, pick.index);
            changed
        }
    }
}

/// *Save the current setup*: a workspace or a layout, its name, what it
/// keeps, Save. True when the settings changed.
fn save_form(ui: &mut egui::Ui, p: &Palette, s: &mut Settings) -> bool {
    let ctx = ui.ctx().clone();
    let mut st = load(&ctx);
    let before = st.clone();
    let mut changed = false;
    ui.vertical(|ui| {
        ui.set_width(ui.available_width().min(LIST_W));
        let mut save = false;
        ui.horizontal(|ui| {
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
                keeps_workspace(ui, p, s, &mut st);
                false
            }
            Kind::Layout => keeps_layout(ui, p, &mut st),
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
                    st.selected = Some(Pick { kind: st.save_kind, index });
                    st.rename = None;
                    st.ask = None;
                    st.refused = None;
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
    });
    if st != before {
        keep(&ctx, st);
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
