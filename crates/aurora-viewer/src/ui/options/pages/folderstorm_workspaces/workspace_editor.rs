//! Folderstorm: a workspace's editor in Preferences › Workspaces › Saved
//! (`crate::folderstorm::workspaces::workspace`, FEATURES.md rows 1.4 and
//! 1.7; folderstorm/plans/workspaces.md, checkpoint 6). Folderstorm (C++):
//! Preferences → Move & View → Workspaces, "Arrange your windows and
//! choose Save current".
//!
//! The parts of the selected workspace's editor ([`super::saved`]): the
//! layout applied with it, the windows it opens (the ticked ones first,
//! the others folded, the additional Inventory windows as one entry), its
//! Inventory folders, and its graphics, camera and HUDs.

use super::{BUTTON_H, ICON_DELETE, PICKER_W, muted};
use crate::camera::CameraPreset;
use crate::folderstorm::workspaces::apply::{self, Live};
use crate::folderstorm::workspaces::context::{self, Camera, Hud};
use crate::folderstorm::workspaces::windows::{INVENTORY_SLOTS, Window};
use crate::folderstorm::workspaces::words::camera_words;
use crate::folderstorm::workspaces::workspace;
use crate::settings::Settings;
use crate::theme::Palette;
use crate::ui::widgets::{self, ActionKind};
use aurora_i18n::tr;
use egui::vec2;

/// The layout applied with the workspace at `index`: "keep the layout in
/// use", or a layout of the list. True when the choice changed.
pub(super) fn layout_picker(ui: &mut egui::Ui, s: &mut Settings, index: usize) -> bool {
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

/// Whether `window` is one of the additional Inventory windows.
fn additional_inventory(window: &Window) -> bool {
    INVENTORY_SLOTS.contains(&window.id)
}

/// The box of one window. True when the workspace's list changed.
fn window_box(ui: &mut egui::Ui, p: &Palette, s: &mut Settings, index: usize, window: &Window) -> bool {
    let store = &mut s.folderstorm_workspaces;
    let mut on = store.workspace(index).is_some_and(|workspace| workspace.open.contains(window.id));
    widgets::checkbox(ui, p, &mut on, window.label().as_ref()).changed() && store.set_workspace_window(index, window.id, on)
}

/// The windows the workspace opens, ticked, first; the others folded
/// under *More windows*, the 16 additional Inventory windows there as one
/// entry. True when its list changed.
pub(super) fn window_boxes(ui: &mut egui::Ui, p: &Palette, s: &mut Settings, index: usize, name: &str) -> bool {
    let mut changed = false;
    muted(ui, p, tr!("fs-workspaces-closes"));
    let opens = |s: &Settings, window: &Window| {
        s.folderstorm_workspaces
            .workspace(index)
            .is_some_and(|workspace| workspace.open.contains(window.id))
    };
    // (the lists of this frame: a box ticked now moves at the next one)
    let ticked: Vec<&Window> = workspace::switchable().filter(|window| opens(s, window)).collect();
    let others: Vec<&Window> = workspace::switchable()
        .filter(|window| !opens(s, window) && !additional_inventory(window))
        .collect();
    let inventories: Vec<&Window> = workspace::switchable()
        .filter(|window| !opens(s, window) && additional_inventory(window))
        .collect();
    if ticked.is_empty() {
        muted(ui, p, tr!("fs-workspaces-holds-none"));
    }
    ui.horizontal_wrapped(|ui| {
        for window in &ticked {
            changed |= window_box(ui, p, s, index, window);
        }
    });
    let more = tr!("fs-workspaces-more-windows", count = others.len() + inventories.len());
    widgets::collapsing(ui, p, ("folderstorm-workspace-windows", name), more, ticked.is_empty(), |ui| {
        ui.horizontal_wrapped(|ui| {
            for window in &others {
                changed |= window_box(ui, p, s, index, window);
            }
        });
        // the additional Inventory windows: one entry, with how many are ticked
        let of = INVENTORY_SLOTS.len();
        let title = tr!("fs-workspaces-more-inventories", ticked = of - inventories.len(), count = of);
        widgets::collapsing(ui, p, ("folderstorm-workspace-inventories", name), title, false, |ui| {
            ui.horizontal_wrapped(|ui| {
                for window in &inventories {
                    changed |= window_box(ui, p, s, index, window);
                }
            });
        });
    });
    changed
}

/// *Remember Inventory folders* of the workspace: ticked, it takes the
/// folder windows open now; unticked, it forgets them. True when changed.
pub(super) fn folders_box(ui: &mut egui::Ui, p: &Palette, s: &mut Settings, index: usize) -> bool {
    let ctx = ui.ctx().clone();
    let mut on = s
        .folderstorm_workspaces
        .workspace(index)
        .is_some_and(|workspace| workspace.folders.is_some());
    let label = tr!("fs-workspaces-remember-folders-now", count = apply::folder_windows_now(&ctx));
    let ticked = widgets::checkbox(ui, p, &mut on, label)
        .on_hover_text(tr!("fs-workspaces-remember-folders-help"))
        .changed();
    ticked && apply::remember_folders(&ctx, &mut Live::of(s), index, on)
}

/// The workspace's context (row 1.7), folded until it has one (C++: every
/// group "Do not change" by default, its save section collapsed): the
/// graphics profile, the camera, the HUDs it wears. True when changed.
pub(super) fn context_section(ui: &mut egui::Ui, p: &Palette, s: &mut Settings, index: usize, name: &str) -> bool {
    let has = s
        .folderstorm_workspaces
        .workspace(index)
        .is_some_and(|workspace| !workspace.context().is_empty());
    let title = match has {
        true => tr!("fs-context-section-set"),
        false => tr!("fs-context-section"),
    };
    widgets::collapsing(ui, p, ("folderstorm-workspace-context", name), title, has, |ui| {
        // (the two pickers in a grid: their boxes line up)
        let mut changed = egui::Grid::new("folderstorm-workspace-context-pickers")
            .num_columns(2)
            .show(ui, |ui| {
                let changed = graphics_picker(ui, s, index);
                ui.end_row();
                let changed = camera_picker(ui, s, index) | changed;
                ui.end_row();
                changed
            })
            .inner;
        changed |= hud_boxes(ui, p, s, index);
        changed
    })
    .body
    .unwrap_or(false)
}

/// The graphics profile applied with the workspace at `index` (row 1.7):
/// "leave the graphics as they are", or a profile of Preferences ›
/// Graphics, by name: its label and its box, two cells of a grid. True
/// when the choice changed.
fn graphics_picker(ui: &mut egui::Ui, s: &mut Settings, index: usize) -> bool {
    let chosen = s
        .folderstorm_workspaces
        .workspace(index)
        .and_then(|workspace| workspace.graphics.clone());
    let gone = chosen.clone().filter(|name| !s.graphics_saved.contains_key(name));
    let keep_label = tr!("fs-context-graphics-keep");
    let shown = match (&chosen, &gone) {
        (None, _) => keep_label.to_owned(),
        (Some(name), Some(_)) => tr!("fs-context-gone", name = name.as_str()),
        (Some(name), None) => name.clone(),
    };
    let mut pick = chosen.clone();
    ui.label(tr!("fs-context-graphics")).on_hover_text(tr!("fs-context-graphics-help"));
    egui::ComboBox::from_id_salt("folderstorm-workspace-graphics")
        .width(PICKER_W)
        .selected_text(shown)
        .truncate()
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut pick, None, keep_label);
            if let Some(name) = &gone {
                ui.selectable_value(&mut pick, Some(name.clone()), tr!("fs-context-gone", name = name.as_str()));
            }
            for name in s.graphics_saved.keys() {
                ui.selectable_value(&mut pick, Some(name.clone()), name.as_str());
            }
        });
    pick != chosen && s.folderstorm_workspaces.set_workspace_graphics(index, pick.as_deref())
}

/// The camera chosen with the workspace at `index` (row 1.7): "leave the
/// camera as it is", one of the four views, or a camera preset by name.
/// Its label and its box, two cells of a grid. True when the choice
/// changed.
fn camera_picker(ui: &mut egui::Ui, s: &mut Settings, index: usize) -> bool {
    let chosen = s
        .folderstorm_workspaces
        .workspace(index)
        .and_then(|workspace| workspace.camera.clone());
    let mut choices: Vec<(Option<Camera>, String)> = vec![(None, tr!("fs-context-camera-keep").to_owned())];
    choices.extend(
        CameraPreset::ALL
            .into_iter()
            .filter(|view| *view != CameraPreset::Custom)
            .map(|view| (Some(Camera::View(view)), view.label().to_owned())),
    );
    for shot in &s.camera_shots {
        let camera = Some(Camera::Preset(shot.name.trim().to_owned()));
        if context::name_ok(&shot.name) && !choices.iter().any(|(value, _)| *value == camera) {
            choices.push((camera, camera_words(&Camera::Preset(shot.name.trim().to_owned()))));
        }
    }
    if let Some(Camera::Preset(name)) = &chosen
        && !choices.iter().any(|(value, _)| value.as_ref() == chosen.as_ref())
    {
        choices.push((
            chosen.clone(),
            tr!("fs-context-gone", name = camera_words(&Camera::Preset(name.clone()))),
        ));
    }
    let shown = choices
        .iter()
        .find(|(value, _)| *value == chosen)
        .map_or_else(String::new, |(_, label)| label.clone());
    let mut pick = chosen.clone();
    ui.label(tr!("fs-context-camera")).on_hover_text(tr!("fs-context-camera-help"));
    egui::ComboBox::from_id_salt("folderstorm-workspace-camera")
        .width(PICKER_W)
        .selected_text(shown)
        .truncate()
        .show_ui(ui, |ui| {
            for (value, label) in &choices {
                ui.selectable_value(&mut pick, value.clone(), label.as_str());
            }
        });
    pick != chosen && s.folderstorm_workspaces.set_workspace_camera(index, pick)
}

/// The HUDs the workspace at `index` wears (row 1.7): a box per HUD it
/// holds (ticked) and per HUD worn now (unticked: tick to add it, with its
/// point). HUDs chosen by another account are counted, with *Forget them*.
/// True when changed.
fn hud_boxes(ui: &mut egui::Ui, p: &Palette, s: &mut Settings, index: usize) -> bool {
    let view = context::view(ui.ctx());
    let Some(huds) = s.folderstorm_workspaces.workspace(index).map(|workspace| workspace.huds.clone()) else {
        return false;
    };
    let mut changed = false;
    ui.add(egui::Label::new(egui::RichText::new(tr!("fs-context-huds")).size(11.5).color(p.muted)).wrap())
        .on_hover_text(tr!("fs-context-huds-help"));
    if let Some(other) = huds.as_ref().filter(|huds| Some(huds.account) != view.account) {
        ui.horizontal_wrapped(|ui| {
            muted(ui, p, tr!("fs-context-huds-other", count = other.items.len()));
            if widgets::action_button(
                ui,
                p,
                ICON_DELETE,
                tr!("fs-context-huds-forget"),
                ActionKind::Flat,
                vec2(0.0, BUTTON_H),
            )
            .clicked()
            {
                changed |= s.folderstorm_workspaces.set_workspace_huds(index, None);
            }
        });
        return changed;
    }
    let Some(account) = view.account else {
        return false;
    };
    let held: Vec<Hud> = huds.map(|huds| huds.items).unwrap_or_default();
    let worn_name = |item: uuid::Uuid| view.worn.iter().find(|(hud, _)| hud.item == item).map(|(_, name)| name.clone());
    let others: Vec<&(Hud, String)> = view
        .worn
        .iter()
        .filter(|(hud, _)| !held.iter().any(|h| h.item == hud.item))
        .collect();
    if held.is_empty() && others.is_empty() {
        muted(ui, p, tr!("fs-context-huds-none"));
    }
    ui.horizontal_wrapped(|ui| {
        for hud in &held {
            let name = view
                .names
                .get(&hud.item)
                .cloned()
                .or_else(|| worn_name(hud.item))
                .unwrap_or_else(|| tr!("fs-context-hud-gone").to_owned());
            let mut on = true;
            if widgets::checkbox(ui, p, &mut on, name).changed() && !on {
                changed |= s.folderstorm_workspaces.set_workspace_hud(index, account, *hud, false);
            }
        }
        for (hud, name) in others {
            let mut on = false;
            if widgets::checkbox(ui, p, &mut on, name.as_str()).changed() && on {
                changed |= s.folderstorm_workspaces.set_workspace_hud(index, account, *hud, true);
            }
        }
    });
    changed
}
