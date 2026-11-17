//! A shortcut's icon style as drawn and edited
//! (folderstorm/plans/margin-desktop.md, "Icon colour and picture"): what
//! its face shows ([`drawn`]), the textures of the pictures, the file
//! dialog, and the window *Shortcut icon* opened from its menu
//! (*Icon › Colour…*, *Inventory texture…*).
//!
//! A copied picture is decoded off the event thread by the backgrounds'
//! own loader (`backgrounds::paint::Loader`, one per picture shown); an
//! Inventory texture is fetched as every interface image is (the app's
//! `ui_images`, asked through [`super::Request::Fetch`]), and only once
//! [`style::texture_asset`] allows it. Neither is drawn for a shortcut
//! whose face RLVa hides, and no Inventory texture is drawn or listed
//! while RLVa hides the Inventory.

use super::Env;
use super::store::Desktop;
use super::style::{self, Picture, Style};
use crate::folderstorm::backgrounds::import::ImportError;
use crate::folderstorm::backgrounds::paint::Loader;
use crate::theme::Palette;
use crate::ui::widgets::{self, ActionKind};
use aurora_i18n::tr;
use egui::{Color32, RichText, Sense, Stroke, Vec2};
use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// What a shortcut's face shows where its icon goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Drawn {
    /// Its icon, in this colour (None: the face's own).
    Icon(Option<Color32>),
    /// A picture: its texture, and its size (for its proportions).
    Picture(egui::TextureId, [f32; 2]),
}

/// The textures of the copied pictures shown, by copy's name.
type Loaders = Arc<Mutex<HashMap<String, Loader>>>;

fn loaders(ctx: &egui::Context) -> Loaders {
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Loaders>(egui::Id::unique("folderstorm_desktop_pictures"))
            .clone()
    })
}

/// What the face of a shortcut of `style` shows now. `hidden`: RLVa hides
/// its face (its own look is not shown). An Inventory texture not fetched
/// yet is asked for in `fetch` (its asset id, never shown).
pub(super) fn drawn(ctx: &egui::Context, account: Option<Uuid>, style: &Style, hidden: bool, env: &Env, fetch: &mut Vec<Uuid>) -> Drawn {
    if hidden {
        return Drawn::Icon(None);
    }
    let colour = style.colour.map(|[r, g, b]| Color32::from_rgb(r, g, b));
    let picture = match &style.picture {
        Some(Picture::File { file, .. }) => account.and_then(|account| {
            let loaders = loaders(ctx);
            let mut loaders = loaders.lock().ok()?;
            let loader = loaders.entry(file.clone()).or_default();
            let (texture, own) = loader.poll(ctx, &style::folder(account), file, style::ICON_SIDE)?;
            Some(Drawn::Picture(texture, [own[0] as f32, own[1] as f32]))
        }),
        // (RlvBehaviourToggleHandler<RLV_BHVR_SHOWINV>: nothing of the
        // Inventory is shown)
        Some(Picture::Texture { texture }) if !env.folders.no_inventory => {
            style::texture_asset(env.folders.inv, *texture).and_then(|asset| match env.images.get(&asset) {
                Some(handle) => {
                    let [w, h] = handle.size();
                    Some(Drawn::Picture(handle.id(), [w as f32, h as f32]))
                }
                None => {
                    fetch.push(asset);
                    None
                }
            })
        }
        _ => None,
    };
    picture.unwrap_or(Drawn::Icon(colour))
}

/// Forgets the textures of the copies no shortcut shows any more
/// (`shown`): their GPU memory goes.
pub(super) fn keep_pictures(ctx: &egui::Context, shown: &BTreeSet<String>) {
    if let Ok(mut loaders) = loaders(ctx).lock()
        && loaders.len() > shown.len()
    {
        loaders.retain(|file, _| shown.contains(file));
    }
}

/// Paints `drawn`'s picture in `icon`, whole, its proportions kept.
pub(super) fn paint_picture(painter: &egui::Painter, icon: egui::Rect, texture: egui::TextureId, size: [f32; 2], greyed: bool) {
    let [w, h] = size;
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let scale = (icon.width() / w).min(icon.height() / h);
    let rect = egui::Rect::from_center_size(icon.center(), egui::vec2(w * scale, h * scale));
    let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    let tint = if greyed {
        widgets::PICTURE_TINT_DISABLED
    } else {
        widgets::PICTURE_TINT
    };
    painter.image(texture, rect, uv, tint);
}

/// What the window and the file dialog ask of the desktop.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Edit {
    Colour(u32, Option<[u8; 3]>),
    Picture(u32, Option<Picture>),
    /// The file dialog, for that shortcut.
    ChooseFile(u32),
    /// Everything of its icon back to its target's.
    Reset(u32),
}

/// The result of a file dialog's thread, once it is done: None when it was
/// cancelled.
type Slot = Arc<Mutex<Option<Option<Result<Picture, ImportError>>>>>;

/// A file dialog open: for which account and shortcut.
#[derive(Clone, Default)]
struct Choosing {
    pending: Option<(Uuid, u32, Slot)>,
}

fn choosing_id() -> egui::Id {
    egui::Id::unique("folderstorm_desktop_icon_choosing")
}

fn choosing(ctx: &egui::Context) -> Arc<Mutex<Choosing>> {
    ctx.data_mut(|d| d.get_temp_mut_or_default::<Arc<Mutex<Choosing>>>(choosing_id()).clone())
}

/// Whether a file dialog is open (its buttons are greyed meanwhile, and no
/// copy is deleted: the one it is writing is not referred to yet).
pub(super) fn dialog_open(ctx: &egui::Context) -> bool {
    choosing(ctx).lock().is_ok_and(|c| c.pending.is_some())
}

/// Opens the file dialog for the shortcut `id` of `account` on its own
/// thread; the picture is checked, reduced and copied there too
/// ([`style::import`]).
pub(super) fn choose_file(ctx: &egui::Context, account: Uuid, id: u32) {
    if dialog_open(ctx) {
        return;
    }
    let slot: Slot = Arc::default();
    let answer = slot.clone();
    let ctx2 = ctx.clone();
    let title = tr!("fs-desktop-icon-file-title");
    let kind = tr!("fs-background-file-type");
    let folder = style::folder(account);
    let spawned = std::thread::Builder::new()
        .name("folderstorm-desktop-icon-dialog".into())
        .spawn(move || {
            // (cancelled: None, nothing to say)
            let result = rfd::FileDialog::new()
                .set_title(title)
                .add_filter(kind, &["png", "jpg", "jpeg", "bmp", "tga", "gif", "webp"])
                .pick_file()
                .map(|path| style::import(&path, &folder));
            if let Some(Err(error)) = &result {
                // (never the path in the log)
                log::info!("desktop icon picture: not taken ({error:?})");
            }
            if let Ok(mut slot) = answer.lock() {
                *slot = Some(result);
            }
            ctx2.request_repaint();
        });
    match spawned {
        Ok(_) => {
            if let Ok(mut choosing) = choosing(ctx).lock() {
                choosing.pending = Some((account, id, slot));
            }
        }
        Err(e) => log::warn!("desktop icon picture: cannot open the file dialog: {e}"),
    }
}

/// The answer of the file dialog, once, for the account logged in: the
/// shortcut it was for and the picture or why not. A dialog cancelled
/// gives nothing; an answer for another account (logged out meanwhile) is
/// dropped.
pub(super) fn chosen(ctx: &egui::Context, account: Option<Uuid>) -> Option<(u32, Result<Picture, ImportError>)> {
    let choosing = choosing(ctx);
    let mut choosing = choosing.lock().ok()?;
    let (for_account, id, slot) = choosing.pending.clone()?;
    let done = slot.lock().ok().and_then(|mut slot| slot.take())?;
    choosing.pending = None;
    let done = done?;
    (account == Some(for_account)).then_some((id, done))
}

/// The answer a test gives in place of the dialog (None: cancelled).
#[cfg(test)]
pub(super) fn answer_dialog(ctx: &egui::Context, account: Uuid, id: u32, result: Option<Result<Picture, ImportError>>) {
    if let Ok(mut choosing) = choosing(ctx).lock() {
        choosing.pending = Some((account, id, Arc::new(Mutex::new(Some(result)))));
    }
}

/// What a refused picture says (the backgrounds' words: the same checks).
pub(super) fn refusal_text(error: ImportError) -> &'static str {
    match error {
        ImportError::TooLarge => tr!("fs-background-error-too-large"),
        ImportError::Unreadable => tr!("fs-background-error-unreadable"),
        ImportError::Disk => tr!("fs-background-error-disk"),
    }
}

/// The window's state: the shortcut it edits, its search, its list.
#[derive(Clone, Default)]
struct Editor {
    id: Option<u32>,
    query: String,
    /// The textures listed for (the inventory's generation, the search).
    listed: Option<(u64, String, Arc<(Vec<(Uuid, String)>, bool)>)>,
    /// Brought to the front once shown.
    raise: bool,
}

fn editor_id() -> egui::Id {
    egui::Id::unique("folderstorm_desktop_icon_editor")
}

/// The window's id (a floater's).
pub(super) const WINDOW: &str = "folderstorm_desktop_icon";

/// Opens the window *Shortcut icon* on the shortcut `id`.
pub(super) fn open(ctx: &egui::Context, id: u32) {
    ctx.data_mut(|d| {
        let editor = d.get_temp_mut_or_default::<Editor>(editor_id());
        if editor.id != Some(id) {
            editor.query.clear();
        }
        editor.id = Some(id);
        editor.raise = true;
    });
}

/// The shortcut the window edits, while it is open.
pub(super) fn editing(ctx: &egui::Context) -> Option<u32> {
    ctx.data(|d| d.get_temp::<Editor>(editor_id())).and_then(|e| e.id)
}

/// Closes the window *Shortcut icon*.
pub(super) fn close(ctx: &egui::Context) {
    ctx.data_mut(|d| d.remove_temp::<Editor>(editor_id()));
}

/// Where the tests find a texture's row in the window.
#[cfg(test)]
pub(super) fn texture_row_id(item: Uuid) -> egui::Id {
    egui::Id::unique(("folderstorm_desktop_texture_row", item))
}

/// Where the tests find a colour of the window.
#[cfg(test)]
pub(super) fn swatch_id(colour: [u8; 3]) -> egui::Id {
    egui::Id::unique(("folderstorm_desktop_swatch", colour))
}

/// The side of a colour of the window.
const SWATCH: f32 = 22.0;
/// The colours of a line.
const PER_ROW: usize = 6;
/// The icon of *Picture from a file…*.
pub(super) const ICON_FILE: &str = "file-upload";

/// The window *Shortcut icon*, while it is open: the colour, the picture
/// (from a file or the Inventory) and *Reset*. `name`: the shortcut's name
/// as its face shows it. What is chosen goes to `edits`.
pub(super) fn window(ctx: &egui::Context, p: &Palette, desktop: &Desktop, env: &Env, name: &str, edits: &mut Vec<Edit>) {
    let Some(mut editor) = ctx.data(|d| d.get_temp::<Editor>(editor_id())) else {
        return;
    };
    let Some(id) = editor.id else {
        return;
    };
    // (gone, or its face hidden by RLVa meanwhile)
    let Some(shortcut) = desktop.shortcut(id) else {
        close(ctx);
        return;
    };
    let style = shortcut.style.clone();
    let custom = shortcut.icon.is_some() || !style.is_default();
    let title = name.to_owned();
    let size = Vec2::new(300.0, 420.0);
    let mut open = true;
    let choosing = dialog_open(ctx);
    widgets::Floater::new(
        WINDOW,
        tr!("fs-desktop-style-title"),
        ctx.content_rect().center() - size / 2.0,
        size,
    )
    .auto_height()
    .show(ctx, p, &mut open, |ui| {
        ui.spacing_mut().item_spacing = Vec2::splat(6.0);
        ui.add(egui::Label::new(RichText::new(&title).strong().color(p.ink)).truncate());
        // the colour
        ui.label(RichText::new(tr!("fs-desktop-style-colour")).color(p.muted));
        for line in style::PRESETS.chunks(PER_ROW) {
            ui.horizontal(|ui| {
                for &colour in line {
                    let resp = swatch(ui, p, colour, style.colour == Some(colour));
                    #[cfg(test)]
                    ui.data_mut(|d| d.insert_temp(swatch_id(colour), resp.rect));
                    if resp.clicked() {
                        edits.push(Edit::Colour(id, Some(colour)));
                    }
                }
            });
        }
        ui.horizontal(|ui| {
            let [r, g, b] = style.colour.unwrap_or(style::PRESETS[7]);
            let mut rgba = [r, g, b, 255];
            if widgets::color_button(ui, p, ("folderstorm_desktop_icon_colour", id), &mut rgba, false).changed() {
                edits.push(Edit::Colour(id, Some([rgba[0], rgba[1], rgba[2]])));
            }
            ui.label(RichText::new(tr!("fs-desktop-style-custom")).color(p.ink));
            let coloured = style.colour.is_some();
            if ui
                .add_enabled_ui(coloured, |ui| widgets::flat_button(ui, p, tr!("fs-desktop-style-colour-default")))
                .inner
                .clicked()
                && coloured
            {
                edits.push(Edit::Colour(id, None));
            }
        });
        if style.picture.is_some() {
            note(ui, p, tr!("fs-desktop-style-colour-picture"));
        }
        widgets::divider(ui, p);
        // the picture
        ui.label(RichText::new(tr!("fs-desktop-style-picture")).color(p.muted));
        let shown = match &style.picture {
            None => (tr!("fs-desktop-style-picture-none").to_owned(), p.muted),
            Some(Picture::File { name, .. }) => (name.clone(), p.ink),
            Some(Picture::Texture { texture })
                if style::texture_asset(env.folders.inv, *texture).is_some() && !env.folders.no_inventory =>
            {
                let name = env.folders.inv.items.get(texture).map(|it| it.name.clone()).unwrap_or_default();
                (name, p.ink)
            }
            Some(Picture::Texture { .. }) => (tr!("fs-desktop-style-texture-gone").to_owned(), p.muted),
            Some(Picture::Kept(_)) => (tr!("fs-desktop-style-picture-other").to_owned(), p.muted),
        };
        ui.add(egui::Label::new(RichText::new(shown.0).color(shown.1)).truncate());
        ui.horizontal_wrapped(|ui| {
            let can = desktop.account.is_some() && !choosing;
            if widgets::action_button_if(
                ui,
                p,
                ICON_FILE,
                tr!("fs-desktop-icon-file"),
                ActionKind::Flat,
                egui::vec2(0.0, 26.0),
                can,
            )
            .clicked()
            {
                edits.push(Edit::ChooseFile(id));
            }
            let pictured = style.picture.is_some();
            if ui
                .add_enabled_ui(pictured, |ui| widgets::flat_button(ui, p, tr!("fs-desktop-style-picture-remove")))
                .inner
                .clicked()
                && pictured
            {
                edits.push(Edit::Picture(id, None));
            }
        });
        note(ui, p, tr!("fs-desktop-style-file-note"));
        widgets::divider(ui, p);
        // a texture of the Inventory
        ui.label(RichText::new(tr!("fs-desktop-style-texture")).color(p.muted));
        if env.folders.no_inventory {
            note(ui, p, tr!("rlva-blocked"));
        } else {
            widgets::search_field(ui, p, &mut editor.query, tr!("fs-desktop-style-search"), ui.available_width());
            let generation = env.folders.inv.generation;
            let fresh = editor
                .listed
                .as_ref()
                .is_some_and(|(at, query, _)| *at == generation && *query == editor.query);
            if !fresh {
                let found = style::offered(env.folders.inv, &editor.query);
                editor.listed = Some((generation, editor.query.clone(), Arc::new(found)));
            }
            let listed = editor.listed.as_ref().map(|(_, _, found)| found.clone()).unwrap_or_default();
            let (found, more) = (&listed.0, listed.1);
            let current = match &style.picture {
                Some(Picture::Texture { texture }) => Some(*texture),
                _ => None,
            };
            if found.is_empty() {
                note(ui, p, tr!("fs-desktop-style-texture-none"));
            } else {
                egui::ScrollArea::vertical()
                    .id_salt("folderstorm_desktop_icon_textures")
                    .max_height(160.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        for (item, name) in found {
                            let resp = widgets::search_list_item(ui, current == Some(*item), name);
                            #[cfg(test)]
                            ui.data_mut(|d| d.insert_temp(texture_row_id(*item), resp.rect));
                            if resp.clicked() {
                                edits.push(Edit::Picture(id, Some(Picture::Texture { texture: *item })));
                            }
                        }
                    });
            }
            if more {
                note(ui, p, &tr!("fs-desktop-style-texture-more", count = style::MAX_LISTED));
            }
            note(ui, p, tr!("fs-desktop-style-texture-note"));
        }
        widgets::divider(ui, p);
        if widgets::action_button_if(
            ui,
            p,
            "restore",
            tr!("fs-desktop-icon-reset"),
            ActionKind::Flat,
            egui::vec2(0.0, 26.0),
            custom,
        )
        .clicked()
        {
            edits.push(Edit::Reset(id));
        }
    });
    if std::mem::take(&mut editor.raise) {
        widgets::bring_floater_front(ctx, WINDOW);
    }
    if open {
        ctx.data_mut(|d| d.insert_temp(editor_id(), editor));
    } else {
        close(ctx);
    }
}

/// A line of help under a part of the window.
fn note(ui: &mut egui::Ui, p: &Palette, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).size(11.0).color(p.muted)).wrap());
}

/// One colour of the window, outlined when the icon has it.
fn swatch(ui: &mut egui::Ui, p: &Palette, [r, g, b]: [u8; 3], chosen: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(SWATCH), Sense::click());
    let radius = ui.visuals().window_corner_radius;
    ui.painter().rect_filled(rect, radius, Color32::from_rgb(r, g, b));
    let edge = if chosen {
        Some(Stroke::new(2.0, p.ink))
    } else if resp.hovered() {
        Some(Stroke::new(1.0, p.violet_light))
    } else {
        None
    };
    if let Some(edge) = edge {
        ui.painter().rect_stroke(rect, radius, edge, egui::StrokeKind::Outside);
    }
    resp.on_hover_text(widgets::color::format_hex([r, g, b, 255], false))
}
