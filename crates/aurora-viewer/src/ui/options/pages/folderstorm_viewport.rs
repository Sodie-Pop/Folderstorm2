//! Folderstorm: Preferences › Layout › Bars and windows › World viewport
//! (`crate::folderstorm::viewport`). Folderstorm (C++) had these under
//! Move & View › Layout.

use super::super::cx::{Cx, field, fields};
use crate::folderstorm::viewport;

pub(super) fn group(cx: &mut Cx) {
    use aurora_i18n::tr;
    cx.group(tr!("folderstorm-viewport"), |cx| {
        cx.toggle(
            tr!("folderstorm-viewport-enable"),
            tr!("folderstorm-viewport-enable-help"),
            &field!(folderstorm_viewport.enabled),
        );
        let off = !cx.s.folderstorm_viewport.enabled;
        cx.subs(off, |cx| {
            let help = tr!("folderstorm-viewport-margin-help");
            cx.slider(
                tr!("folderstorm-viewport-left"),
                help,
                &field!(folderstorm_viewport.left),
                0..=16384,
                " px",
            );
            cx.slider(
                tr!("folderstorm-viewport-right"),
                help,
                &field!(folderstorm_viewport.right),
                0..=16384,
                " px",
            );
            cx.slider(
                tr!("folderstorm-viewport-top"),
                help,
                &field!(folderstorm_viewport.top),
                0..=16384,
                " px",
            );
            cx.slider(
                tr!("folderstorm-viewport-bottom"),
                help,
                &field!(folderstorm_viewport.bottom),
                0..=16384,
                " px",
            );
            cx.row(
                tr!("folderstorm-viewport-fit"),
                tr!("folderstorm-viewport-fit-help"),
                &fields!(
                    folderstorm_viewport.left,
                    folderstorm_viewport.right,
                    folderstorm_viewport.top,
                    folderstorm_viewport.bottom
                ),
                |ui, s| monitor_buttons(ui, &mut s.folderstorm_viewport),
            );
            cx.toggle(
                tr!("folderstorm-viewport-mouselook"),
                tr!("folderstorm-viewport-mouselook-help"),
                &field!(folderstorm_viewport.in_mouselook),
            );
            cx.toggle(
                tr!("folderstorm-viewport-bars"),
                tr!("folderstorm-viewport-bars-help"),
                &field!(folderstorm_viewport.bars_in_viewport),
            );
        });
    });
}

/// One button per monitor under the window: its margins put the world on it.
fn monitor_buttons(ui: &mut egui::Ui, s: &mut viewport::ViewportSettings) -> bool {
    let Some(layout) = viewport::monitors(ui.ctx()) else {
        ui.label(aurora_i18n::tr!("folderstorm-viewport-no-monitors"));
        return false;
    };
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        let mut any = false;
        for (n, monitor) in layout.monitors.iter().enumerate() {
            let Some((left, right, top, bottom)) = viewport::margins_for_monitor(layout.window, *monitor) else {
                continue;
            };
            any = true;
            let label = aurora_i18n::tr!("folderstorm-viewport-monitor", n = n + 1, width = monitor[2], height = monitor[3]);
            if ui.button(label).clicked() {
                (s.left, s.right, s.top, s.bottom) = (left, right, top, bottom);
                changed = true;
            }
        }
        if !any {
            ui.label(aurora_i18n::tr!("folderstorm-viewport-no-monitors"));
        }
    });
    changed
}
