//! Folderstorm: Preferences › Workspaces › World viewport
//! (`crate::folderstorm::viewport`), and the Margins tab beside it.
//! Folderstorm (C++) had these under Move & View › Layout. The diagram
//! (`crate::folderstorm::diagram`) shows under the switch as soon as the
//! viewport is on, with the bars' place; the margins as numbers, the
//! monitors' buttons and the other switches stay behind the cog, as the
//! keyboard route, and edit the same settings. The Margins tab holds what
//! the margins show: their background ([`background`]) and their desktop
//! ([`desktop`]), behind one notice while the viewport is off.

pub(super) mod background;
mod desktop;

use super::super::cx::{Cx, Nothing, field, fields};
use super::folderstorm_search::words;
use crate::folderstorm::viewport::bars::BarsPlace;
use crate::folderstorm::{diagram, viewport};
use crate::ui::widgets::{self, ActionKind};
use aurora_i18n::tr;

/// The embedded Tabler icon of a monitor's button (as Graphics' page).
const ICON_MONITOR: &str = "device-desktop-filled";
/// The largest margin, in pixels.
const MARGIN_MAX: u32 = 16384;

/// *World viewport*: its switch, the bars' place and the diagram; behind
/// the cog, the margins as numbers, the monitors and two switches.
pub(super) fn group(cx: &mut Cx) {
    cx.group(tr!("folderstorm-viewport"), |cx| {
        words(cx, &["fs-search-viewport"]).toggle(
            tr!("folderstorm-viewport-enable"),
            tr!("folderstorm-viewport-enable-help"),
            &field!(folderstorm_viewport.enabled),
        );
        let off = !cx.s.folderstorm_viewport.enabled;
        cx.subs(off, |cx| {
            let help = tr!("folderstorm-viewport-margin-help");
            for (label, margin) in [
                (tr!("folderstorm-viewport-left"), field!(folderstorm_viewport.left)),
                (tr!("folderstorm-viewport-right"), field!(folderstorm_viewport.right)),
                (tr!("folderstorm-viewport-top"), field!(folderstorm_viewport.top)),
                (tr!("folderstorm-viewport-bottom"), field!(folderstorm_viewport.bottom)),
            ] {
                // a number: 0–16384 is too wide for a slider to set exactly
                let get = margin.get_mut;
                words(cx, &["fs-search-viewport"]).row(label, help, &margin, |ui, s| {
                    ui.add(
                        egui::DragValue::new(get(s))
                            .custom_parser(aurora_i18n::parse_number)
                            .range(0..=MARGIN_MAX)
                            .speed(4.0)
                            .suffix(" px"),
                    )
                    .changed()
                });
            }
            let p = cx.p;
            words(cx, &["fs-search-viewport"]).wide().row(
                tr!("folderstorm-viewport-fit"),
                tr!("folderstorm-viewport-fit-help"),
                &fields!(
                    folderstorm_viewport.left,
                    folderstorm_viewport.right,
                    folderstorm_viewport.top,
                    folderstorm_viewport.bottom
                ),
                |ui, s| monitor_buttons(ui, &p, &mut s.folderstorm_viewport),
            );
            words(cx, &["fs-search-viewport"]).toggle(
                tr!("folderstorm-viewport-mouselook"),
                tr!("folderstorm-viewport-mouselook-help"),
                &field!(folderstorm_viewport.in_mouselook),
            );
            words(cx, &["fs-search-viewport", "fs-search-photo"]).toggle(
                tr!("folderstorm-viewport-photo"),
                tr!("folderstorm-viewport-photo-help"),
                &field!(folderstorm_viewport.photo_whole_window),
            );
        });
        // the rest only matters while the world leaves room around it
        cx.greyed(off);
        // inside the world, around it, or over the whole window
        // (`viewport::bars`); kept in two switches of the settings
        let p = cx.p;
        let places = [
            (BarsPlace::Inside, tr!("folderstorm-viewport-bars-inside"), false),
            (BarsPlace::Around, tr!("folderstorm-viewport-bars-around"), false),
            (BarsPlace::Window, tr!("folderstorm-viewport-bars-window"), false),
        ];
        let choices: Vec<&str> = places.iter().map(|(_, label, _)| *label).collect();
        words(cx, &["fs-search-viewport"]).keywords(&choices.join(" ")).wide().row(
            tr!("folderstorm-viewport-bars"),
            tr!("folderstorm-viewport-bars-help"),
            &fields!(folderstorm_viewport.bars_in_viewport, folderstorm_viewport.bars_around),
            |ui, s| {
                let mut place = BarsPlace::of(&s.folderstorm_viewport);
                let changed = super::super::segmented(ui, &p, &mut place, &places);
                if changed {
                    place.set(&mut s.folderstorm_viewport);
                }
                changed
            },
        );
        // the diagram shows as soon as the viewport is on, without opening
        // the cog
        if !off {
            words(cx, &["fs-search-viewport"]).below().row(
                tr!("folderstorm-viewport-diagram"),
                tr!("folderstorm-viewport-diagram-help"),
                &fields!(
                    folderstorm_viewport.left,
                    folderstorm_viewport.right,
                    folderstorm_viewport.top,
                    folderstorm_viewport.bottom
                ),
                |ui, s| {
                    let desktop = diagram::desktop(ui.ctx());
                    diagram::widget::show(ui, &p, &mut s.folderstorm_viewport, &desktop)
                },
            );
        }
    });
}

/// The Margins tab: one notice while the viewport is off, then what the
/// margins show (row 1.8) and their shortcuts (row 1.10), greyed until
/// the viewport leaves them room.
pub(super) fn margins(cx: &mut Cx) {
    let off = !cx.s.folderstorm_viewport.enabled;
    if off {
        let p = cx.p;
        cx.group(tr!("folderstorm-viewport"), |cx| {
            words(cx, &["fs-search-viewport"])
                .wide()
                .row(tr!("fs-margins-off"), tr!("fs-margins-off-help"), &Nothing, |ui, s| {
                    let clicked = widgets::action_button(
                        ui,
                        &p,
                        ICON_MONITOR,
                        tr!("fs-margins-turn-on"),
                        ActionKind::Flat,
                        egui::vec2(0.0, 26.0),
                    )
                    .clicked();
                    if clicked {
                        s.folderstorm_viewport.enabled = true;
                    }
                    clicked
                });
        });
    }
    background::group(cx, off);
    desktop::group(cx, off);
}

/// One button per monitor under the window: its margins put the world on it.
fn monitor_buttons(ui: &mut egui::Ui, p: &crate::theme::Palette, s: &mut viewport::ViewportSettings) -> bool {
    let Some(layout) = viewport::monitors(ui.ctx()) else {
        ui.label(tr!("folderstorm-viewport-no-monitors"));
        return false;
    };
    let mut changed = false;
    // (one line: a wrapped one ran into the next row)
    ui.horizontal(|ui| {
        let mut any = false;
        for (n, monitor) in layout.monitors.iter().enumerate() {
            let Some((left, right, top, bottom)) = viewport::margins_for_monitor(layout.window, *monitor) else {
                continue;
            };
            any = true;
            let label = tr!("folderstorm-viewport-monitor", n = n + 1, width = monitor[2], height = monitor[3]);
            if widgets::action_button(ui, p, ICON_MONITOR, &label, ActionKind::Flat, egui::vec2(0.0, 26.0)).clicked() {
                (s.left, s.right, s.top, s.bottom) = (left, right, top, bottom);
                changed = true;
            }
        }
        if !any {
            ui.label(tr!("folderstorm-viewport-no-monitors"));
        }
    });
    changed
}
