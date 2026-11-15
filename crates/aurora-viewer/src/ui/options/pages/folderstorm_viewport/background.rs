//! Folderstorm: the background of the margins in Preferences
//! (`crate::folderstorm::backgrounds`, FEATURES.md row 1.8). Folderstorm
//! (C++) had it under Move & View › Backgrounds; here the default is a
//! group of Layout › Bars and windows, after *World viewport*, and a
//! layout's own is chosen under the selected layout in Layout › Layouts &
//! workspaces ([`layout_part`]).

use super::super::super::cx::{Cx, field};
use crate::folderstorm::backgrounds::import::{self, ImportError};
use crate::folderstorm::backgrounds::paint::{self, Status};
use crate::folderstorm::backgrounds::record::{Background, Fit, Kind, Picture};
use crate::settings::Settings;
use crate::theme::Palette;
use crate::ui::widgets::{self, ActionKind};
use aurora_i18n::tr;
use std::sync::{Arc, Mutex};

/// The embedded Tabler icon of *Choose a picture…*.
const ICON_PICTURE: &str = "photo-filled";

fn kinds() -> [(Kind, &'static str, bool); 3] {
    [
        (Kind::None, tr!("fs-background-kind-none"), false),
        (Kind::Colour, tr!("fs-background-kind-colour"), false),
        (Kind::Picture, tr!("fs-background-kind-picture"), false),
    ]
}

fn fits() -> [(Fit, &'static str); 5] {
    [
        (Fit::Fill, tr!("fs-background-fit-fill")),
        (Fit::Fit, tr!("fs-background-fit-fit")),
        (Fit::Stretch, tr!("fs-background-fit-stretch")),
        (Fit::Centre, tr!("fs-background-fit-centre")),
        (Fit::Tile, tr!("fs-background-fit-tile")),
    ]
}

/// *Background around the world*: the default, for the layouts without
/// their own.
pub(in crate::ui::options) fn group(cx: &mut Cx) {
    let p = cx.p;
    cx.group(tr!("fs-background-group"), |cx| {
        if !cx.s.folderstorm_viewport.enabled {
            cx.note(tr!("fs-background-intro"));
        }
        // (Folderstorm (C++)'s words: "wallpaper", "background", "monitor")
        cx.keywords("wallpaper background margins monitors desktop").choice(
            tr!("fs-background-kind"),
            tr!("fs-background-kind-help"),
            &field!(folderstorm_workspaces.background.kind),
            &kinds(),
        );
        let kind = cx.s.folderstorm_workspaces.background.kind;
        cx.greyed(kind != Kind::Picture);
        cx.keywords("wallpaper image photo file").wide().row(
            tr!("fs-background-picture"),
            tr!("fs-background-picture-help"),
            &field!(folderstorm_workspaces.background.picture),
            |ui, s| picture_row(ui, &p, s, &Target::Default),
        );
        cx.combo(
            tr!("fs-background-fit"),
            tr!("fs-background-fit-help"),
            &field!(folderstorm_workspaces.background.fit),
            &fits(),
        );
        cx.keywords("span spanning wallpaper").toggle(
            tr!("fs-background-span"),
            tr!("fs-background-span-help"),
            &field!(folderstorm_workspaces.background.span),
        );
        cx.greyed(kind == Kind::None);
        cx.color(
            tr!("fs-background-colour"),
            tr!("fs-background-colour-help"),
            &field!(folderstorm_workspaces.background.colour),
        );
        cx.greyed(kind != Kind::Picture);
        cx.keywords("dim dimming darker").slider(
            tr!("fs-background-brightness"),
            tr!("fs-background-brightness-help"),
            &field!(folderstorm_workspaces.background.brightness),
            crate::folderstorm::backgrounds::record::BRIGHTNESS,
            " %",
        );
        cx.greyed(false);
    });
}

/// The background a chosen picture goes to.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Target {
    Default,
    /// The layout of that name.
    Layout(String),
}

fn background_of<'a>(s: &'a mut Settings, target: &Target) -> Option<&'a mut Background> {
    let store = &mut s.folderstorm_workspaces;
    match target {
        Target::Default => Some(&mut store.background),
        Target::Layout(name) => {
            let index = store.find(name)?.0;
            match store.layouts.get_mut(index)? {
                crate::folderstorm::workspaces::store::Record::Layout(layout) => layout.background.as_mut(),
                crate::folderstorm::workspaces::store::Record::Kept(_) => None,
            }
        }
    }
}

/// The result of a file dialog's thread.
type Slot = Arc<Mutex<Option<Result<Picture, ImportError>>>>;

/// A file dialog open, and the last refusal, for the session.
#[derive(Default)]
struct Choosing {
    pending: Option<(Target, Slot)>,
    /// The frame the pending choice was last looked at: one not looked at
    /// for a few frames (Preferences closed, another tab, a search) is
    /// dropped, so that a late answer never replaces what is shown.
    looked: u64,
    refused: Option<(Target, ImportError)>,
}

fn choosing(ctx: &egui::Context) -> Arc<Mutex<Choosing>> {
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Arc<Mutex<Choosing>>>(egui::Id::unique("folderstorm_background_choosing"))
            .clone()
    })
}

/// Opens the file dialog on its own thread; the picture is checked and
/// copied there too (`import::import`).
fn open_dialog(ctx: &egui::Context, target: Target) {
    let slot: Slot = Arc::default();
    let answer = slot.clone();
    let ctx2 = ctx.clone();
    let title = tr!("fs-background-choose-title");
    let kind = tr!("fs-background-file-type");
    let spawned = std::thread::Builder::new()
        .name("folderstorm-background-dialog".into())
        .spawn(move || {
            let Some(path) = rfd::FileDialog::new()
                .set_title(title)
                .add_filter(kind, &["png", "jpg", "jpeg", "bmp", "tga", "gif", "webp"])
                .pick_file()
            else {
                return;
            };
            let result = import::import(&path, &import::folder());
            if let Err(error) = result {
                // (never the path in the log)
                log::info!("background picture: not taken ({error:?})");
            }
            if let Ok(mut slot) = answer.lock() {
                *slot = Some(result);
            }
            ctx2.request_repaint();
        });
    match spawned {
        Ok(_) => {
            if let Ok(mut choosing) = choosing(ctx).lock() {
                choosing.pending = Some((target, slot));
                choosing.looked = ctx.cumulative_frame_nr();
                choosing.refused = None;
            }
        }
        Err(e) => log::warn!("background picture: cannot open the file dialog: {e}"),
    }
}

/// The answer of the dialog opened for `target`, once, while it is still
/// awaited; whether one is awaited.
fn answer(ctx: &egui::Context, target: &Target) -> (Option<Result<Picture, ImportError>>, bool) {
    let choosing = choosing(ctx);
    let Ok(mut choosing) = choosing.lock() else {
        return (None, false);
    };
    let now = ctx.cumulative_frame_nr();
    if choosing.looked.saturating_add(3) < now {
        choosing.pending = None;
    }
    choosing.looked = now;
    let Some((for_target, slot)) = &choosing.pending else {
        return (None, false);
    };
    if for_target != target {
        return (None, true);
    }
    let done = slot.lock().ok().and_then(|mut slot| slot.take());
    let waiting = done.is_none();
    if !waiting {
        choosing.pending = None;
        choosing.refused = match &done {
            Some(Err(error)) => Some((target.clone(), *error)),
            _ => None,
        };
    }
    (done, waiting)
}

fn refused(ctx: &egui::Context, target: &Target) -> Option<ImportError> {
    let choosing = choosing(ctx);
    let choosing = choosing.lock().ok()?;
    choosing.refused.as_ref().filter(|(t, _)| t == target).map(|(_, e)| *e)
}

fn refusal_text(error: ImportError) -> &'static str {
    match error {
        ImportError::TooLarge => tr!("fs-background-error-too-large"),
        ImportError::Unreadable => tr!("fs-background-error-unreadable"),
        ImportError::Disk => tr!("fs-background-error-disk"),
    }
}

/// *Choose a picture…*, the picture's name or what is wrong with it. A
/// picture chosen becomes the background's, "A picture" is chosen, and
/// the copies no background refers to any more are deleted.
fn picture_row(ui: &mut egui::Ui, p: &Palette, s: &mut Settings, target: &Target) -> bool {
    let ctx = ui.ctx().clone();
    let mut changed = false;
    let (done, waiting) = answer(&ctx, target);
    if waiting {
        ctx.request_repaint_after_secs(0.2);
    }
    if let Some(Ok(picture)) = done
        && let Some(background) = background_of(s, target)
    {
        background.picture = Some(picture);
        background.kind = Kind::Picture;
        changed = true;
        paint::forget(&ctx);
        import::prune(
            &import::folder(),
            &crate::folderstorm::backgrounds::referenced(&s.folderstorm_workspaces),
        );
    }
    let Some(background) = background_of(s, target) else {
        return changed;
    };
    ui.horizontal_wrapped(|ui| {
        if widgets::action_button_if(
            ui,
            p,
            ICON_PICTURE,
            tr!("fs-background-choose"),
            ActionKind::Flat,
            egui::vec2(0.0, 26.0),
            !waiting,
        )
        .clicked()
        {
            open_dialog(&ctx, target.clone());
        }
        let shown = match &background.picture {
            None => (tr!("fs-background-none-chosen").to_owned(), p.muted),
            Some(picture) => match paint::status(&ctx, &picture.file) {
                Status::Loading => (format!("{} · {}", picture.name, tr!("fs-background-loading")), p.muted),
                _ => (picture.name.clone(), p.ink),
            },
        };
        ui.add(egui::Label::new(egui::RichText::new(shown.0).size(12.5).color(shown.1)).truncate());
    });
    let problem = match &background.picture {
        Some(picture) => match paint::status(&ctx, &picture.file) {
            Status::Missing => Some(tr!("fs-background-missing")),
            Status::Unreadable => Some(tr!("fs-background-unreadable")),
            _ => None,
        },
        None => None,
    };
    if let Some(text) = refused(&ctx, target).map(refusal_text).or(problem) {
        ui.add(egui::Label::new(egui::RichText::new(text).size(11.5).color(p.danger)).wrap());
    }
    changed
}

/// The selected layout's background, under its actions (Layout ›
/// Layouts & workspaces): *Use the default* or *Its own*, and its own's
/// choices. True when the layout changed.
pub(in crate::ui::options) fn layout_part(ui: &mut egui::Ui, p: &Palette, s: &mut Settings, index: usize) -> bool {
    let Some(layout) = s.folderstorm_workspaces.layout(index) else {
        return false;
    };
    let name = layout.name.clone();
    let mut own = layout.background.is_some();
    let mut changed = false;
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tr!("fs-background-layout")).size(12.5).color(p.ink))
            .on_hover_text(tr!("fs-background-layout-help"));
        let choices = [
            (false, tr!("fs-background-layout-default"), false),
            (true, tr!("fs-background-layout-own"), false),
        ];
        if super::super::super::segmented(ui, p, &mut own, &choices) {
            // its own starts as a copy of the default
            let background = own.then(|| s.folderstorm_workspaces.background.clone());
            changed |= s.folderstorm_workspaces.set_layout_background(index, background);
        }
    });
    if !own {
        return changed;
    }
    let target = Target::Layout(name);
    let id = ui.scope_id().with(("folderstorm-background-layout", index));
    egui::Grid::new(id).num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
        let Some(mut edited) = background_of(s, &target).cloned() else {
            return;
        };
        let label = |ui: &mut egui::Ui, text: &str, help: &str| {
            ui.label(egui::RichText::new(text).size(12.5).color(p.ink)).on_hover_text(help);
        };
        let mut edits = false;
        label(ui, tr!("fs-background-shows"), tr!("fs-background-kind-help"));
        edits |= super::super::super::segmented(ui, p, &mut edited.kind, &kinds());
        ui.end_row();
        let picture = edited.kind == Kind::Picture;
        label(ui, tr!("fs-background-picture"), tr!("fs-background-picture-help"));
        let mut chosen = false;
        ui.add_enabled_ui(picture, |ui| {
            ui.vertical(|ui| chosen = picture_row(ui, p, s, &target));
        });
        ui.end_row();
        if chosen {
            // the picture went into the layout itself: start from it
            if let Some(fresh) = background_of(s, &target) {
                edited.picture = fresh.picture.clone();
                edited.kind = fresh.kind;
            }
            changed = true;
        }
        label(ui, tr!("fs-background-fit"), tr!("fs-background-fit-help"));
        ui.add_enabled_ui(picture, |ui| edits |= super::super::super::combo(ui, &mut edited.fit, &fits()));
        ui.end_row();
        label(ui, tr!("fs-background-span"), tr!("fs-background-span-help"));
        ui.add_enabled_ui(picture, |ui| edits |= super::super::super::toggle(ui, p, &mut edited.span));
        ui.end_row();
        label(ui, tr!("fs-background-colour"), tr!("fs-background-colour-help"));
        ui.add_enabled_ui(edited.kind != Kind::None, |ui| {
            edits |= super::super::super::swatch(ui, p, ("folderstorm-background-colour", index), &mut edited.colour)
        });
        ui.end_row();
        label(ui, tr!("fs-background-brightness"), tr!("fs-background-brightness-help"));
        ui.add_enabled_ui(picture, |ui| {
            edits |= super::super::super::slider(
                ui,
                &mut edited.brightness,
                crate::folderstorm::backgrounds::record::BRIGHTNESS,
                " %",
            )
        });
        ui.end_row();
        if edits {
            changed |= s.folderstorm_workspaces.set_layout_background(index, Some(edited));
        }
    });
    changed
}
