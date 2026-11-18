//! Folderstorm: the margins' desktop in Preferences
//! (`crate::folderstorm::desktop`, FEATURES.md row 1.10): a group of
//! Workspaces › Margins, after *Background around the world*.

use super::super::super::cx::{Cx, Nothing, field};
use super::super::folderstorm_search::words;
use crate::folderstorm::desktop;
use crate::folderstorm::desktop::store::Size;
use crate::folderstorm::desktop::surface::size_label;
use crate::ui::widgets::{self, ActionKind};
use aurora_i18n::tr;
use egui::vec2;

/// The height of the group's buttons.
const BUTTON_H: f32 = 26.0;

/// *Remove all shortcuts…* was clicked: its question shows.
fn asking_id() -> egui::Id {
    egui::Id::unique("folderstorm_desktop_clear_asking")
}

/// *Desktop in the margins*: the shortcuts' switch (behind its cog: their
/// size, how they open, and removing them all), the note about the
/// shortcuts of a monitor not in use, and the taskbar; greyed while the
/// viewport is `off`. The shortcuts themselves are the account's, not
/// settings: the rows about them have nothing to reset.
pub(in crate::ui::options) fn group(cx: &mut Cx, off: bool) {
    let p = cx.p;
    cx.group(tr!("fs-desktop-group"), |cx| {
        cx.greyed(off);
        words(cx, &["fs-search-desktop"]).toggle(
            tr!("fs-desktop-shortcuts"),
            tr!("fs-desktop-shortcuts-help"),
            &field!(folderstorm_desktop.shortcuts_on),
        );
        let shortcuts_off = !cx.s.folderstorm_desktop.shortcuts_on;
        cx.subs(shortcuts_off, |cx| {
            let sizes = Size::ALL.map(|size| (size, size_label(size), false));
            words(cx, &["fs-search-desktop"]).choice(
                tr!("fs-desktop-size"),
                tr!("fs-desktop-size-help"),
                &field!(folderstorm_desktop.size),
                &sizes,
            );
            words(cx, &["fs-search-desktop"]).toggle(
                tr!("fs-desktop-single-click"),
                tr!("fs-desktop-single-click-help"),
                &field!(folderstorm_desktop.single_click),
            );
            clear_row(cx);
        });
        // shortcuts kept on a monitor that has no desktop now (the row
        // exists only then: it is a state, not a setting to search for)
        let ctx = cx.ui.ctx().clone();
        let unused = desktop::unused(&ctx, &cx.s.folderstorm_desktop);
        if unused > 0 {
            cx.wide().row(
                &tr!("fs-desktop-unused", count = unused),
                tr!("fs-desktop-unused-help"),
                &Nothing,
                |ui, s| {
                    let bring = widgets::action_button(
                        ui,
                        &p,
                        "arrow-right",
                        tr!("fs-desktop-bring"),
                        ActionKind::Flat,
                        vec2(0.0, BUTTON_H),
                    );
                    bring.clicked() && {
                        let brought = desktop::bring_here(ui.ctx(), &mut s.folderstorm_desktop);
                        brought.moved + brought.merged > 0
                    }
                },
            );
        }
        taskbar_rows(cx);
    });
}

/// The taskbar: its switch and, behind its cog, the monitor it stands on
/// and what it lists.
fn taskbar_rows(cx: &mut Cx) {
    words(cx, &["fs-search-taskbar"]).toggle(
        tr!("fs-taskbar-switch"),
        tr!("fs-taskbar-switch-help"),
        &field!(folderstorm_desktop.taskbar.on),
    );
    let off = !cx.s.folderstorm_desktop.taskbar.on;
    cx.subs(off, |cx| {
        // the monitors that have room for it now, and the one chosen
        // even when it has none (it is kept, and said so by its help)
        let ctx = cx.ui.ctx().clone();
        let mut slots = desktop::live_slots(&ctx, &cx.s.folderstorm_desktop);
        slots.extend(cx.s.folderstorm_desktop.taskbar.slot);
        slots.sort_unstable();
        slots.dedup();
        let numbers: Vec<String> = slots.iter().map(|slot| (u32::from(*slot) + 1).to_string()).collect();
        let mut choices: Vec<(Option<u8>, &str, bool)> = vec![(None, tr!("fs-taskbar-monitor-auto"), false)];
        choices.extend(
            slots
                .iter()
                .zip(&numbers)
                .map(|(slot, number)| (Some(*slot), number.as_str(), false)),
        );
        words(cx, &["fs-search-taskbar"]).choice(
            tr!("fs-taskbar-monitor"),
            tr!("fs-taskbar-monitor-help"),
            &field!(folderstorm_desktop.taskbar.slot),
            &choices,
        );
        words(cx, &["fs-search-taskbar"]).toggle(
            tr!("fs-taskbar-all"),
            tr!("fs-taskbar-all-help"),
            &field!(folderstorm_desktop.taskbar.all_windows),
        );
    });
}

/// *Remove all shortcuts…*: asks in its row before removing.
fn clear_row(cx: &mut Cx) {
    let p = cx.p;
    let count = cx.s.folderstorm_desktop.shortcuts.len();
    words(cx, &["fs-search-desktop"])
        .wide()
        .row(tr!("fs-desktop-clear"), tr!("fs-desktop-clear-help"), &Nothing, |ui, s| {
            let asking = count > 0 && ui.data(|d| d.get_temp::<bool>(asking_id())).unwrap_or(false);
            let size = vec2(0.0, BUTTON_H);
            let mut answer = None;
            let mut removed = false;
            if asking {
                ui.label(egui::RichText::new(tr!("fs-desktop-clear-ask", count = count)).color(p.ink));
                if widgets::action_button(ui, &p, "trash-filled", tr!("fs-desktop-clear-confirm"), ActionKind::Danger, size).clicked() {
                    removed = s.folderstorm_desktop.clear() > 0;
                    answer = Some(false);
                }
                if widgets::action_button(ui, &p, "x", tr!("common-cancel"), ActionKind::Flat, size).clicked() {
                    answer = Some(false);
                }
            } else if widgets::action_button_if(
                ui,
                &p,
                "trash-filled",
                tr!("fs-desktop-clear-button"),
                ActionKind::Flat,
                size,
                count > 0,
            )
            .clicked()
            {
                answer = Some(true);
            }
            if let Some(asking) = answer {
                ui.data_mut(|d| d.insert_temp(asking_id(), asking));
            }
            removed
        });
}
