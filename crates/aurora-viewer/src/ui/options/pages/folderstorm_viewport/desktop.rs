//! Folderstorm: the margins' desktop in Preferences
//! (`crate::folderstorm::desktop`, FEATURES.md row 1.10): a group of
//! Layout › Bars and windows, after *Background around the world*.

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

/// *Desktop in the margins*: the shortcuts' switch, their size, how they
/// open, and what tidies them (the shortcuts themselves are the account's:
/// they are not settings, so these rows have nothing to reset).
pub(in crate::ui::options) fn group(cx: &mut Cx) {
    let p = cx.p;
    cx.group(tr!("fs-desktop-group"), |cx| {
        if !cx.s.folderstorm_viewport.enabled {
            cx.note(tr!("fs-desktop-intro"));
        }
        words(cx, &["fs-search-desktop"]).toggle(
            tr!("fs-desktop-shortcuts"),
            tr!("fs-desktop-shortcuts-help"),
            &field!(folderstorm_desktop.shortcuts_on),
        );
        let off = !cx.s.folderstorm_desktop.shortcuts_on;
        cx.greyed(off);
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
        cx.greyed(false);
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
        let count = cx.s.folderstorm_desktop.shortcuts.len();
        words(cx, &["fs-search-desktop"])
            .wide()
            .row(tr!("fs-desktop-clear"), tr!("fs-desktop-clear-help"), &Nothing, |ui, s| {
                let asking = count > 0 && ui.data(|d| d.get_temp::<bool>(asking_id())).unwrap_or(false);
                let mut answer = None;
                let mut removed = false;
                if asking {
                    ui.label(egui::RichText::new(tr!("fs-desktop-clear-ask", count = count)).color(p.ink));
                    let size = vec2(0.0, BUTTON_H);
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
                    vec2(0.0, BUTTON_H),
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
    });
}
