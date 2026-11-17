//! Folderstorm: Preferences › Workspaces › Switching: every standing
//! preference about switching in one place (the favorites strip, the two
//! layouts of the *Switch layout* button, what happens when the monitors
//! change, the previous workspace's HUDs), each tracked like any other
//! setting; then the three ways back, side by side.

use super::super::super::cx::{Custom, Cx, Nothing, field};
use super::super::folderstorm_search::words;
use super::saved;
use super::{BUTTON_H, ICON_BACK, ICON_RESET, ICON_UNLOCK, PICKER_W, muted};
use crate::folderstorm::workspaces::apply::{self, Live};
use crate::folderstorm::workspaces::store::MonitorPolicy;
use crate::settings::Settings;
use crate::theme::Palette;
use crate::ui::widgets::{self, ActionKind};
use aurora_i18n::tr;
use egui::vec2;

pub(super) fn page(cx: &mut Cx) {
    let p = cx.p;
    cx.group(tr!("fs-switcher-favorites"), |cx| {
        cx.note(tr!("fs-switching-favorites-intro"));
        // (Folderstorm (C++): "Show workspace favorites strip", off until chosen)
        words(cx, &["fs-search-layouts-quick"])
            .keywords("strip row bar pills landmark star")
            .toggle(
                tr!("fs-strip-show"),
                tr!("fs-strip-show-help"),
                &field!(folderstorm_workspaces.strip),
            );
    });
    cx.group(tr!("fs-switching-toggle"), |cx| {
        cx.note(tr!("fs-switching-toggle-intro"));
        for (place, label) in [(0, tr!("fs-layouts-toggle-first")), (1, tr!("fs-layouts-toggle-second"))] {
            // each of the two its own modified dot; reset to "not chosen"
            let track = Custom {
                path: "folderstorm_workspaces.toggle",
                modified: move |s: &Settings, d: &Settings| {
                    s.folderstorm_workspaces.toggle[place] != d.folderstorm_workspaces.toggle[place]
                },
                reset: move |s: &mut Settings, d: &Settings| {
                    s.folderstorm_workspaces.toggle[place] = d.folderstorm_workspaces.toggle[place].clone();
                },
            };
            words(cx, &["fs-search-layouts-quick"]).row(label, tr!("fs-layouts-toggle-help"), &track, |ui, s| toggle_picker(ui, s, place));
        }
        // its keys, and the other layouts' keys, on the Shortcuts page
        let other = super::super::shortcuts::tabs().len().saturating_sub(1);
        words(cx, &["fs-search-layouts-quick"]).link(
            tr!("fs-switching-keys"),
            tr!("fs-switching-keys-help"),
            tr!("fs-switching-keys-button"),
            super::super::super::Page::Shortcuts,
            other,
        );
    });
    cx.group(tr!("fs-switching-auto"), |cx| {
        let policies = [
            (MonitorPolicy::Ask, tr!("fs-layouts-monitors-ask"), false),
            (MonitorPolicy::Apply, tr!("fs-layouts-monitors-apply"), false),
            (MonitorPolicy::Nothing, tr!("fs-layouts-monitors-nothing"), false),
        ];
        words(cx, &["fs-search-layouts-monitors"]).choice(
            tr!("fs-layouts-monitors"),
            tr!("fs-layouts-monitors-help"),
            &field!(folderstorm_workspaces.on_monitor_change),
            &policies,
        );
        // (row 1.7: on until unticked; the store keeps the opposite, so
        // that a file without it takes them off)
        words(cx, &["fs-search-workspaces-huds"]).row(
            tr!("fs-context-take-off"),
            tr!("fs-context-take-off-help"),
            &field!(folderstorm_workspaces.keep_huds),
            |ui, s| {
                let mut on = !s.folderstorm_workspaces.keep_huds;
                let changed = super::super::super::toggle(ui, &p, &mut on);
                s.folderstorm_workspaces.keep_huds = !on;
                changed
            },
        );
    });
    cx.group(tr!("fs-switching-back"), |cx| {
        words(
            cx,
            &["fs-search-layouts-previous", "fs-search-layouts-lock", "fs-search-layouts-reset"],
        )
        .section_body()
        .row(tr!("fs-switching-back"), tr!("fs-switching-back-help"), &Nothing, |ui, s| {
            get_back(ui, &p, s)
        });
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

/// *Previous arrangement*, *Unlock the viewer window* and *Reset bars and
/// world* in one line, each greyed until it has something to do, its
/// help on hover; what the first did, under them. True when the settings
/// changed.
fn get_back(ui: &mut egui::Ui, p: &Palette, s: &mut Settings) -> bool {
    let ctx = ui.ctx().clone();
    let size = vec2(0.0, BUTTON_H);
    let mut changed = false;
    let mut went_back = None;
    ui.vertical(|ui| {
        ui.horizontal_wrapped(|ui| {
            let help = tr!("fs-layouts-previous-help");
            let previous = widgets::action_button_if(
                ui,
                p,
                ICON_BACK,
                tr!("fs-layouts-previous"),
                ActionKind::Flat,
                size,
                apply::has_previous(&ctx),
            )
            .on_hover_text(help)
            .on_disabled_hover_text(help);
            if previous.clicked() && apply::previous(&ctx, &mut Live::of(s)) {
                went_back = Some(true);
                changed = true;
            }
            let help = tr!("fs-layouts-unlock-help");
            let held = s.folderstorm_workspaces.window_held;
            let unlock = widgets::action_button_if(ui, p, ICON_UNLOCK, tr!("fs-layouts-unlock"), ActionKind::Flat, size, held)
                .on_hover_text(help)
                .on_disabled_hover_text(help);
            if unlock.clicked() && apply::unlock(&mut s.folderstorm_workspaces) {
                changed = true;
            }
            let help = tr!("fs-layouts-reset-help");
            let world = s.folderstorm_viewport != Default::default();
            let reset = widgets::action_button_if(ui, p, ICON_RESET, tr!("fs-layouts-reset"), ActionKind::Flat, size, world)
                .on_hover_text(help)
                .on_disabled_hover_text(help);
            if reset.clicked() && apply::reset_world(&ctx, &mut Live::of(s)) {
                went_back = Some(false);
                changed = true;
            }
        });
        if saved::load(&ctx).went_back && went_back.is_none() {
            muted(ui, p, tr!("fs-layouts-previous-done"));
        }
    });
    if let Some(back) = went_back {
        let mut st = saved::load(&ctx);
        st.went_back = back;
        // the entry in use is the one selected
        st.selected = None;
        saved::keep(&ctx, st);
    }
    changed
}
