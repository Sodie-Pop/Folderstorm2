//! Folderstorm: the *Workspaces* groups of Preferences › Layout › Layouts &
//! workspaces (`crate::folderstorm::workspaces::workspace`, FEATURES.md row
//! 1.4; folderstorm/plans/workspaces.md, checkpoint 6). Folderstorm (C++):
//! Preferences → Move & View → Workspaces, "Arrange your windows and
//! choose Save current".
//!
//! The list of saved workspaces, the actions on the selected one (Apply,
//! Update, Rename, Delete, Move up and down, its star), the layout applied
//! with it and a box per window it opens; and *Save as a new workspace*.
//! As for layouts, everything applies at once, Update and Delete ask first,
//! and "Reset this page" never deletes a workspace.

use super::super::super::cx::{Cx, Nothing};
use super::{
    Ask, BUTTON_H, ICON_CANCEL, ICON_CHECK, ICON_DELETE, ICON_DOWN, ICON_RENAME, ICON_SAVE, ICON_STAR, ICON_UP, LIST_W, PICKER_W, entry, muted,
    warning,
};
use crate::folderstorm::workspaces::apply::{self, Live};
use crate::folderstorm::workspaces::store::{MAX_NAME, StoreError, Unusable};
use crate::folderstorm::workspaces::words::{report_text, workspace_holds, workspace_refusal};
use crate::folderstorm::workspaces::workspace::{self, Record};
use crate::settings::Settings;
use crate::theme::Palette;
use crate::ui::widgets::{self, ActionKind};
use aurora_i18n::tr;
use egui::vec2;

/// What was last refused, to say why under the field it came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Refused {
    Save(StoreError),
    Selected(StoreError),
}

/// The groups' own state, for the session (egui's temporary data).
#[derive(Clone, Debug, Default, PartialEq)]
struct Tab {
    /// The entry selected in the list.
    selected: Option<usize>,
    /// The name typed for a new workspace, and whether it names the layout
    /// in use.
    new_name: String,
    with_layout: bool,
    /// The name being typed for the selected workspace.
    rename: Option<String>,
    ask: Option<Ask>,
    refused: Option<Refused>,
}

fn tab_id() -> egui::Id {
    egui::Id::unique("folderstorm_workspaces_tab")
}

fn load(ctx: &egui::Context) -> Tab {
    ctx.data(|d| d.get_temp::<Tab>(tab_id())).unwrap_or_default()
}

fn keep(ctx: &egui::Context, tab: Tab) {
    ctx.data_mut(|d| d.insert_temp(tab_id(), tab));
}

fn rename_id() -> egui::Id {
    egui::Id::unique("folderstorm_workspaces_rename")
}

pub(super) fn groups(cx: &mut Cx) {
    let p = cx.p;
    cx.group(tr!("fs-workspaces-group"), |cx| {
        cx.note(tr!("fs-workspaces-intro"));
        // (the search word of Folderstorm (C++): "workspace")
        cx.keywords("workspace workspaces windows open close activity sorting exploring")
            .section_body()
            .row(tr!("fs-workspaces-list"), tr!("fs-workspaces-list-help"), &Nothing, |ui, s| {
                list(ui, &p, s)
            });
        cx.keywords("workspace save windows open").section_body().row(
            tr!("fs-workspaces-save-new"),
            tr!("fs-workspaces-save-help"),
            &Nothing,
            |ui, s| save_new(ui, &p, s),
        );
    });
}

fn active_index(s: &Settings) -> Option<usize> {
    let store = &s.folderstorm_workspaces;
    (0..store.workspaces.len()).find(|index| store.is_active_workspace(*index))
}

/// The note at the right of a workspace's line.
fn note(ctx: &egui::Context, s: &Settings, index: usize, record: &Record) -> &'static str {
    let store = &s.folderstorm_workspaces;
    match record {
        Record::Kept(kept) => match kept.why {
            Unusable::Newer => tr!("fs-layouts-newer"),
            Unusable::Invalid => tr!("fs-layouts-invalid"),
        },
        Record::Workspace(_) if !store.is_active_workspace(index) => "",
        Record::Workspace(_) if apply::workspace_modified(store, &apply::open_now(ctx)) => tr!("fs-layouts-modified"),
        Record::Workspace(_) => tr!("fs-layouts-active"),
    }
}

/// The list and the selected workspace's actions. True when the settings
/// changed.
fn list(ui: &mut egui::Ui, p: &Palette, s: &mut Settings) -> bool {
    let ctx = ui.ctx().clone();
    let mut tab = load(&ctx);
    let before = tab.clone();
    let count = s.folderstorm_workspaces.workspaces.len();
    if tab.selected.is_some_and(|index| index >= count) {
        tab.selected = None;
    }
    if tab.selected.is_none() {
        tab.selected = active_index(s);
        tab.rename = None;
        tab.ask = None;
    }
    let mut changed = false;
    ui.vertical(|ui| {
        ui.set_width(ui.available_width().min(LIST_W));
        ui.spacing_mut().item_spacing.y = 4.0;
        if count == 0 {
            muted(ui, p, tr!("fs-workspaces-empty"));
        }
        for index in 0..count {
            let store = &s.folderstorm_workspaces;
            let record = &store.workspaces[index];
            let name = record.name().unwrap_or(tr!("fs-layouts-unnamed"));
            let selected = tab.selected == Some(index);
            let usable = record.workspace().is_some();
            let line = entry(
                ui,
                p,
                name,
                selected,
                store.is_active_workspace(index),
                usable,
                false,
                usable && store.is_favorite_workspace(name),
                note(&ctx, s, index, record),
            );
            if line.clicked() && !selected {
                tab.selected = Some(index);
                tab.rename = None;
                tab.ask = None;
                tab.refused = None;
            }
            if line.double_clicked() && usable {
                changed |= do_apply(&ctx, s, &mut tab, index);
            }
        }
        if let Some(index) = tab.selected {
            ui.add_space(4.0);
            changed |= selected_actions(ui, p, s, &mut tab, index);
        }
        // what the last apply of a workspace did
        if let Some(report) = apply::report(&ctx).filter(|report| report.workspace.is_some()) {
            muted(ui, p, report_text(&report, s.folderstorm_workspaces.pending.len()));
        }
    });
    if tab != before {
        keep(&ctx, tab);
    }
    changed
}

fn do_apply(ctx: &egui::Context, s: &mut Settings, tab: &mut Tab, index: usize) -> bool {
    tab.ask = None;
    tab.rename = None;
    match apply::apply_workspace(ctx, &mut Live::of(s), index) {
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

/// The layout applied with the workspace at `index`: "keep the layout in
/// use", or a layout of the list. True when the choice changed.
fn layout_picker(ui: &mut egui::Ui, s: &mut Settings, index: usize) -> bool {
    let store = &mut s.folderstorm_workspaces;
    let mut choices: Vec<(Option<usize>, &str)> = vec![(None, tr!("fs-workspaces-layout-keep"))];
    choices.extend(
        store
            .layouts
            .iter()
            .enumerate()
            .filter_map(|(at, record)| Some((Some(at), record.layout()?.name.as_str()))),
    );
    let chosen = store
        .workspace(index)
        .and_then(|workspace| workspace.layout.as_deref())
        .and_then(|name| store.find(name))
        .map(|(at, _)| at);
    let mut pick = chosen;
    let shown = choices.iter().find(|(value, _)| *value == chosen).map_or("", |(_, label)| *label);
    ui.horizontal(|ui| {
        ui.label(tr!("fs-workspaces-layout"));
        egui::ComboBox::from_id_salt("folderstorm-workspace-layout")
            .width(PICKER_W)
            .selected_text(shown)
            .truncate()
            .show_ui(ui, |ui| {
                for (value, label) in &choices {
                    ui.selectable_value(&mut pick, *value, *label);
                }
            });
    });
    if pick == chosen {
        return false;
    }
    let name = pick.and_then(|at| store.layout(at)).map(|layout| layout.name.clone());
    store.set_workspace_layout(index, name.as_deref())
}

/// A box per window the workspace may open. True when its list changed.
fn window_boxes(ui: &mut egui::Ui, p: &Palette, s: &mut Settings, index: usize) -> bool {
    let mut changed = false;
    muted(ui, p, tr!("fs-workspaces-windows"));
    ui.horizontal_wrapped(|ui| {
        for window in workspace::switchable() {
            let store = &mut s.folderstorm_workspaces;
            let mut on = store.workspace(index).is_some_and(|workspace| workspace.open.contains(window.id));
            if widgets::checkbox(ui, p, &mut on, window.label()).changed() {
                changed |= store.set_workspace_window(index, window.id, on);
            }
        }
    });
    changed
}

/// Under the list: what the selected entry does and what can be done with
/// it. True when the settings changed.
fn selected_actions(ui: &mut egui::Ui, p: &Palette, s: &mut Settings, tab: &mut Tab, index: usize) -> bool {
    let ctx = ui.ctx().clone();
    let Some(record) = s.folderstorm_workspaces.workspaces.get(index).cloned() else {
        return false;
    };
    let name = record.name().unwrap_or(tr!("fs-layouts-unnamed")).to_owned();
    let usable = record.workspace().is_some();
    let count = s.folderstorm_workspaces.workspaces.len();
    let size = vec2(0.0, BUTTON_H);
    let mut changed = false;
    match &record {
        Record::Workspace(workspace) => muted(ui, p, workspace_holds(workspace)),
        Record::Kept(kept) => muted(
            ui,
            p,
            match kept.why {
                Unusable::Newer => tr!("fs-workspaces-newer-help"),
                Unusable::Invalid => tr!("fs-workspaces-invalid-help"),
            },
        ),
    }
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
            if widgets::action_button(ui, p, ICON_CHECK, tr!("fs-layouts-rename"), ActionKind::Primary, size).clicked() || enter {
                done = Some(true);
            }
            if widgets::action_button(ui, p, ICON_CANCEL, tr!("common-cancel"), ActionKind::Flat, size).clicked() {
                done = Some(false);
            }
        });
        match done {
            Some(true) => match s.folderstorm_workspaces.rename_workspace(index, typed) {
                Ok(()) => {
                    tab.rename = None;
                    tab.refused = None;
                    changed = true;
                }
                Err(error) => tab.refused = Some(Refused::Selected(error)),
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
                tr!("fs-workspaces-ask-update", name = name.as_str()),
                ICON_SAVE,
                tr!("fs-layouts-update"),
                ActionKind::Primary,
            ),
            Ask::Delete => (
                tr!("fs-workspaces-ask-delete", name = name.as_str()),
                ICON_DELETE,
                tr!("common-delete"),
                ActionKind::Danger,
            ),
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
        if let Some(yes) = answer {
            tab.ask = None;
            if yes {
                match ask {
                    Ask::Update => match apply::update_workspace(&ctx, &mut Live::of(s), index) {
                        Ok(()) => {
                            tab.refused = None;
                            changed = true;
                        }
                        Err(error) => tab.refused = Some(Refused::Selected(error)),
                    },
                    Ask::Delete => {
                        changed |= s.folderstorm_workspaces.remove_workspace(index);
                        tab.selected = None;
                        tab.refused = None;
                    }
                }
            }
        }
    } else {
        ui.horizontal_wrapped(|ui| {
            if widgets::action_button_if(ui, p, ICON_CHECK, tr!("fs-layouts-apply"), ActionKind::Primary, size, usable).clicked() {
                changed |= do_apply(&ctx, s, tab, index);
            }
            if widgets::action_button_if(ui, p, ICON_SAVE, tr!("fs-layouts-update"), ActionKind::Flat, size, usable)
                .on_hover_text(tr!("fs-workspaces-update-help"))
                .clicked()
            {
                tab.ask = Some(Ask::Update);
            }
            if widgets::action_button_if(ui, p, ICON_RENAME, tr!("fs-layouts-rename"), ActionKind::Flat, size, usable).clicked() {
                use egui::text::{CCursor, CCursorRange};
                let mut state = egui::TextEdit::load_state(&ctx, rename_id()).unwrap_or_default();
                let all = CCursorRange::two(CCursor::new(0), CCursor::new(name.chars().count()));
                state.cursor.set_char_range(Some(all));
                state.store(&ctx, rename_id());
                ctx.memory_mut(|m| m.request_focus(rename_id()));
                tab.rename = Some(name.clone());
            }
            if widgets::action_button(ui, p, ICON_DELETE, tr!("common-delete"), ActionKind::FlatDanger, size).clicked() {
                tab.ask = Some(Ask::Delete);
            }
            if widgets::icon_button(ui, p, ICON_UP, tr!("fs-layouts-move-up"), index > 0)
                && let Some(to) = s.folderstorm_workspaces.shift_workspace(index, true)
            {
                tab.selected = Some(to);
                changed = true;
            }
            if widgets::icon_button(ui, p, ICON_DOWN, tr!("fs-layouts-move-down"), index + 1 < count)
                && let Some(to) = s.folderstorm_workspaces.shift_workspace(index, false)
            {
                tab.selected = Some(to);
                changed = true;
            }
            let favorite = usable && s.folderstorm_workspaces.is_favorite_workspace(&name);
            let star_tip = match favorite {
                true => tr!("fs-switcher-unstar"),
                false => tr!("fs-switcher-star"),
            };
            if usable && widgets::icon_toggle(ui, p, ICON_STAR, star_tip, favorite) {
                changed |= s.folderstorm_workspaces.set_favorite_workspace(&name, !favorite);
            }
        });
    }
    if let Some(Refused::Selected(error)) = tab.refused {
        warning(ui, p, workspace_refusal(error));
    }
    if tab.rename.is_none() && tab.ask.is_none() && usable {
        changed |= layout_picker(ui, s, index);
        changed |= window_boxes(ui, p, s, index);
    }
    changed
}

/// *Save as a new workspace*: the windows open now under a name, with the
/// layout in use when asked. True when the settings changed.
fn save_new(ui: &mut egui::Ui, p: &Palette, s: &mut Settings) -> bool {
    let ctx = ui.ctx().clone();
    let mut tab = load(&ctx);
    let before = tab.clone();
    let mut changed = false;
    ui.vertical(|ui| {
        ui.set_width(ui.available_width().min(LIST_W));
        let mut save = false;
        ui.horizontal(|ui| {
            let field = ui.add(
                egui::TextEdit::singleline(&mut tab.new_name)
                    .desired_width(240.0)
                    .char_limit(MAX_NAME * 2)
                    .hint_text(tr!("fs-workspaces-name-hint")),
            );
            save |= field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            save |= widgets::action_button(ui, p, ICON_SAVE, tr!("fs-workspaces-save"), ActionKind::Primary, vec2(0.0, BUTTON_H)).clicked();
        });
        let layout = s.folderstorm_workspaces.active().map(|layout| layout.name.clone());
        ui.add_enabled_ui(layout.is_some(), |ui| {
            let mut with = tab.with_layout && layout.is_some();
            let label = match &layout {
                Some(name) => tr!("fs-workspaces-with-layout", name = name.as_str()),
                None => tr!("fs-workspaces-with-no-layout").to_owned(),
            };
            if widgets::checkbox(ui, p, &mut with, label).changed() {
                tab.with_layout = with;
            }
        });
        if save {
            let with_layout = tab.with_layout && layout.is_some();
            match apply::save_new_workspace(&ctx, &mut Live::of(s), &tab.new_name, with_layout) {
                Ok(index) => {
                    tab.new_name.clear();
                    tab.selected = Some(index);
                    tab.rename = None;
                    tab.ask = None;
                    tab.refused = None;
                    changed = true;
                }
                Err(error) => tab.refused = Some(Refused::Save(error)),
            }
        }
        if let Some(Refused::Save(error)) = tab.refused {
            warning(ui, p, workspace_refusal(error));
        }
    });
    if tab != before {
        keep(&ctx, tab);
    }
    changed
}
